//! Group records by a key, and apply a reduction function.
//!
//! The `group` operators act on data that can be viewed as pairs `(key, val)`. They group records
//! with the same key, and apply user supplied functions to the key and a list of values, which are
//! expected to populate a list of output values.
//!
//! Several variants of `group` exist which allow more precise control over how grouping is done.
//! For example, the `_by` suffixed variants take arbitrary data, but require a key-value selector
//! to be applied to each record. The `_u` suffixed variants use unsigned integers as keys, and
//! will use a dense array rather than a `HashMap` to store their keys.
//!
//! The list of values are presented as an iterator which internally merges sorted lists of values.
//! This ordering can be exploited in several cases to avoid computation when only the first few
//! elements are required.
//!
//! #Examples
//!
//! This example groups a stream of `(key,val)` pairs by `key`, and yields only the most frequently
//! occurring value for each key.
//!
//! ```ignore
//! stream.group(|key, vals, output| {
//!     let (mut max_val, mut max_wgt) = vals.peek().unwrap();
//!     for (val, wgt) in vals {
//!         if wgt > max_wgt {
//!             max_wgt = wgt;
//!             max_val = val;
//!         }
//!     }
//!     output.push((max_val.clone(), max_wgt));
//! })
//! ```

use std::rc::Rc;
use std::default::Default;
use std::hash::{Hash, Hasher};
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::DerefMut;

use itertools::Itertools;

use ::Data;
use timely::dataflow::*;
use timely::dataflow::operators::{Map, Binary};
use timely::dataflow::channels::pact::Exchange;
use timely::drain::DrainExt;

use collection::{LeastUpperBound, Lookup, Trace, Offset};
use collection::trace::CollectionIterator;

use iterators::coalesce::Coalesce;
use radix_sort::{RadixSorter, Unsigned};
use collection::compact::Compact;

/// Extension trait for the `group_by` and `group_by_u` differential dataflow methods.
pub trait CoGroupBy<G: Scope, K: Data, V1: Data> : Binary<G, ((K,V1), i32)>+Map<G, ((K,V1), i32)>
where G::Timestamp: LeastUpperBound {

    /// The lowest level `cogroup` implementation, which is parameterized by the type of storage to
    /// use for mapping keys `K` to `Offset`, an internal `CollectionTrace` type. This method should
    /// probably rarely be used directly.
    fn cogroup_by_inner<
        D:     Data,
        V2:    Data+Default,
        V3:    Data+Default,
        U:     Unsigned+Default,
        KH:    Fn(&K)->U+'static,
        Look:  Lookup<K, Offset>+'static,
        LookG: Fn(u64)->Look,
        Logic: Fn(&K, &mut CollectionIterator<V1>, &mut CollectionIterator<V2>, &mut Vec<(V3, i32)>)+'static,
        Reduc: Fn(&K, &V3)->D+'static,
    >
    (&self, other: &Stream<G, ((K,V2),i32)>, key_h: KH, reduc: Reduc, look: LookG, logic: Logic) -> Stream<G, (D, i32)> {

        let mut source1 = Trace::new(look(0));
        let mut source2 = Trace::new(look(0));
        let mut result = Trace::new(look(0));

        // A map from times to received (key, val, wgt) triples.
        let mut inputs1 = Vec::new();
        let mut inputs2 = Vec::new();

        // A map from times to a list of keys that need processing at that time.
        let mut to_do = Vec::new();

        // temporary storage for operator implementations to populate
        let mut buffer = vec![];
        let mut heap1 = vec![];
        let mut heap2 = vec![];
        let mut heap3 = vec![];

        let key_h = Rc::new(key_h);
        let key_1 = key_h.clone();
        let key_2 = key_h.clone();

        // create an exchange channel based on the supplied Fn(&D1)->u64.
        let exch1 = Exchange::new(move |&(ref x,_)| key_1(&x.0).as_u64());
        let exch2 = Exchange::new(move |&(ref x,_)| key_2(&x.0).as_u64());

        let mut sorter1 = RadixSorter::new();
        let mut sorter2 = RadixSorter::new();

        // fabricate a data-parallel operator using the `unary_notify` pattern.
        self.binary_notify(other, exch1, exch2, "CoGroupBy", vec![], move |input1, input2, output, notificator| {

            // 1. read each input, and stash it in our staging area
            while let Some((time, data)) = input1.next() {
                notificator.notify_at(&time);
                inputs1.entry_or_insert(time.clone(), || Vec::new())
                       .push(::std::mem::replace(data.deref_mut(), Vec::new()));
            }

            // 1. read each input, and stash it in our staging area
            while let Some((time, data)) = input2.next() {
                notificator.notify_at(&time);
                inputs2.entry_or_insert(time.clone(), || Vec::new())
                       .push(::std::mem::replace(data.deref_mut(), Vec::new()));
            }

            // 2. go through each time of interest that has reached completion
            // times are interesting either because we received data, or because we conclude
            // in the processing of a time that a future time will be interesting.
            while let Some((index, _count)) = notificator.next() {

                // 2a. fetch any data associated with this time.
                if let Some(mut queue) = inputs1.remove_key(&index) {

                    // sort things; radix if many, .sort_by if few.
                    let compact = if queue.len() > 1 {
                        for element in queue.into_iter() {
                            sorter1.extend(element.into_iter(), &|x| key_h(&(x.0).0));
                        }
                        let mut sorted = sorter1.finish(&|x| key_h(&(x.0).0));
                        let result = Compact::from_radix(&mut sorted, &|k| key_h(k));
                        sorted.truncate(256);
                        sorter1.recycle(sorted);
                        result
                    }
                    else {
                        let mut vec = queue.pop().unwrap();
                        let mut vec = vec.drain_temp().collect::<Vec<_>>();
                        vec.sort_by(|x,y| key_h(&(x.0).0).cmp(&key_h((&(y.0).0))));
                        Compact::from_radix(&mut vec![vec], &|k| key_h(k))
                    };

                    if let Some(compact) = compact {

                        for key in &compact.keys {
                            for time in source1.interesting_times(key, index.clone()).iter() {
                                let mut queue = to_do.entry_or_insert((*time).clone(), || { notificator.notify_at(time); Vec::new() });
                                queue.push((*key).clone());
                            }
                        }

                        source1.set_difference(index.clone(), compact);
                    }
                }

                // 2a. fetch any data associated with this time.
                if let Some(mut queue) = inputs2.remove_key(&index) {

                    // sort things; radix if many, .sort_by if few.
                    let compact = if queue.len() > 1 {
                        for element in queue.into_iter() {
                            sorter2.extend(element.into_iter(), &|x| key_h(&(x.0).0));
                        }
                        let mut sorted = sorter2.finish(&|x| key_h(&(x.0).0));
                        let result = Compact::from_radix(&mut sorted, &|k| key_h(k));
                        sorted.truncate(256);
                        sorter2.recycle(sorted);
                        result
                    }
                    else {
                        let mut vec = queue.pop().unwrap();
                        let mut vec = vec.drain_temp().collect::<Vec<_>>();
                        vec.sort_by(|x,y| key_h(&(x.0).0).cmp(&key_h((&(y.0).0))));
                        Compact::from_radix(&mut vec![vec], &|k| key_h(k))
                    };

                    if let Some(compact) = compact {

                        for key in &compact.keys {
                            for time in source2.interesting_times(key, index.clone()).iter() {
                                let mut queue = to_do.entry_or_insert((*time).clone(), || { notificator.notify_at(time); Vec::new() });
                                queue.push((*key).clone());
                            }
                        }

                        source2.set_difference(index.clone(), compact);
                    }
                }

                // we may need to produce output at index
                let mut session = output.session(&index);


                    // 2b. We must now determine for each interesting key at this time, how does the
                    // currently reported output match up with what we need as output. Should we send
                    // more output differences, and what are they?

                // Much of this logic used to hide in `OperatorTrace` and `CollectionTrace`.
                // They are now gone and simpler, respectively.
                if let Some(mut keys) = to_do.remove_key(&index) {

                    // we would like these keys in a particular order.
                    // TODO : use a radix sort since we have `key_h`.
                    keys.sort_by(|x,y| (key_h(&x), x).cmp(&(key_h(&y), y)));
                    keys.dedup();

                    // accumulations for installation into result
                    let mut accumulation = Compact::new(0,0);

                    for key in keys {

                        // acquire an iterator over the collection at `time`.
                        let mut input1 = unsafe { source1.get_collection_using(&key, &index, &mut heap1) };
                        let mut input2 = unsafe { source2.get_collection_using(&key, &index, &mut heap2) };

                        // if we have some data, invoke logic to populate self.dst
                        if input1.peek().is_some() || input2.peek().is_some() { logic(&key, &mut input1, &mut input2, &mut buffer); }

                        buffer.sort_by(|x,y| x.0.cmp(&y.0));

                        // push differences in to Compact.
                        let mut compact = accumulation.session();
                        for (val, wgt) in Coalesce::coalesce(unsafe { result.get_collection_using(&key, &index, &mut heap3) }
                                                                   .map(|(v, w)| (v,-w))
                                                                   .merge_by(buffer.iter().map(|&(ref v, w)| (v, w)), |x,y| {
                                                                        x.0 <= y.0
                                                                   }))
                        {
                            session.give((reduc(&key, val), wgt));
                            compact.push(val.clone(), wgt);
                        }
                        compact.done(key);
                        buffer.clear();
                    }

                    if accumulation.vals.len() > 0 {
                        // println!("group2");
                        result.set_difference(index.clone(), accumulation);
                    }
                }
            }
        })
    }
}
