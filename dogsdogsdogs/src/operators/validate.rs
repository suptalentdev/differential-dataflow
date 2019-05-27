use std::collections::HashMap;
use std::ops::Mul;
use std::hash::Hash;

use timely::PartialOrder;
use timely::dataflow::Scope;
use timely::dataflow::channels::pact::{Pipeline, Exchange};
use timely::dataflow::operators::Operator;

use timely_sort::Unsigned;

use differential_dataflow::{ExchangeData, Collection, AsCollection, Hashable};
use differential_dataflow::difference::Monoid;
use differential_dataflow::lattice::Lattice;
use differential_dataflow::operators::arrange::Arranged;
use differential_dataflow::trace::{Cursor, TraceReader, BatchReader};

/// Proposes extensions to a stream of prefixes.
///
/// This method takes a stream of prefixes and for each determines a
/// key with `key_selector` and then proposes all pair af the prefix
/// and values associated with the key in `arrangement`.
pub fn validate<G, K, V, Tr, F, P>(
    extensions: &Collection<G, (P, V), Tr::R>,
    arrangement: Arranged<G, Tr>,
    key_selector: F,
) -> Collection<G, (P, V), Tr::R>
where
    G: Scope,
    G::Timestamp: Lattice,
    Tr: TraceReader<Key=(K,V), Val=(), Time=G::Timestamp>+Clone+'static,
    K: Ord+Hash+Clone+Default,
    V: ExchangeData+Hash+Default,
    Tr::Batch: BatchReader<Tr::Key, Tr::Val, Tr::Time, Tr::R>,
    Tr::Cursor: Cursor<Tr::Key, Tr::Val, Tr::Time, Tr::R>,
    Tr::R: Monoid+Mul<Output = Tr::R>+ExchangeData,
    F: Fn(&P)->K+Clone+'static,
    P: ExchangeData,
{
    use operators::propose::propose_then;
    propose_then(
        extensions,
        arrangement,
        move |(pre,val),key| { *key = (key_selector(pre), val.clone()); },
        |(pre,val),&()| (pre.clone(), val.clone()),
    )
    // validate_with(
    //     extensions,
    //     arrangement,
    //     Rc::new(|data, key, value| *key = key_selector(data.0); *value = data.1),
    // )
}

// /// and values associated with the key in `arrangement`.
// pub fn validate_with<G, K, V, Tr, F, D>(
//     extensions: &Collection<G, D, Tr::R>,
//     arrangement: Arranged<G, Tr>,
//     key_selector: Rc<F>,
// ) -> Collection<G, (P, V), Tr::R>
// where
//     G: Scope,
//     G::Timestamp: Lattice,
//     Tr: TraceReader<Key=(K,V), Val=(), Time=G::Timestamp>+Clone+'static,
//     K: Ord+Hash+Clone,
//     V: ExchangeData+Hash,
//     Tr::Batch: BatchReader<Tr::Key, Tr::Val, Tr::Time, Tr::R>,
//     Tr::Cursor: Cursor<Tr::Key, Tr::Val, Tr::Time, Tr::R>,
//     Tr::R: Monoid+Mul<Output = Tr::R>+ExchangeData,
//     F: Fn(&D, &mut K, &mut V)+'static,
//     D: ExchangeData,
// {
//     let validate_stream = arrangement.stream;
//     let mut validate_trace = Some(arrangement.trace);

//     let mut stash = HashMap::new();
//     let logic1 = key_selector.clone();
//     let logic2 = key_selector.clone();

//     let mut buffer1 = Vec::new();
//     let mut buffer2 = Vec::new();

//     let exchange = Exchange::new(move |update: &((P,V),G::Timestamp,Tr::R)|
//         (logic1(&(update.0).0).clone(), ((update.0).1).clone()).hashed().as_u64()
//     );

//     extensions.inner.binary_frontier(&validate_stream, exchange, Pipeline, "Validate", move |_,_| move |input1, input2, output| {

//         // drain the first input, stashing requests.
//         input1.for_each(|capability, data| {
//             data.swap(&mut buffer1);
//             stash.entry(capability.retain())
//                  .or_insert(Vec::new())
//                  .extend(buffer1.drain(..))
//         });

//         // advance the `distinguish_since` frontier to allow all merges.
//         input2.for_each(|_, batches| {
//             batches.swap(&mut buffer2);
//             for batch in buffer2.drain(..) {
//                 if let Some(ref mut trace) = validate_trace {
//                     trace.distinguish_since(batch.upper());
//                 }
//             }
//         });

//         if let Some(ref mut trace) = validate_trace {

//             for (capability, prefixes) in stash.iter_mut() {

//                 // defer requests at incomplete times.
//                 // NOTE: not all updates may be at complete times, but if this test fails then none of them are.
//                 if !input2.frontier.less_equal(capability.time()) {

//                     let mut session = output.session(capability);

//                     // sort requests for in-order cursor traversal. could consolidate?
//                     prefixes.sort_by(|x,y| (logic2(&(x.0).0), &((x.0).1)).cmp(&(logic2(&(y.0).0), &((y.0).1))));

//                     let (mut cursor, storage) = trace.cursor();

//                     for &mut (ref prefix, ref time, ref mut diff) in prefixes.iter_mut() {
//                         if !input2.frontier.less_equal(time) {
//                             let key = (logic2(&prefix.0), (prefix.1).clone());
//                             cursor.seek_key(&storage, &key);
//                             if cursor.get_key(&storage) == Some(&key) {
//                                 let mut count = Tr::R::zero();
//                                 cursor.map_times(&storage, |t, d| if t.less_equal(time) { count += d; });
//                                 let prod = count * diff.clone();
//                                 if !prod.is_zero(){
//                                     session.give((prefix.clone(), time.clone(), prod));
//                                 }
//                             }
//                             *diff = Tr::R::zero();
//                         }
//                     }

//                     prefixes.retain(|ptd| !ptd.2.is_zero());
//                 }
//             }
//         }

//         // drop fully processed capabilities.
//         stash.retain(|_,prefixes| !prefixes.is_empty());

//         // advance the consolidation frontier (TODO: wierd lexicographic times!)
//         validate_trace.as_mut().map(|trace| trace.advance_by(&input1.frontier().frontier()));

//         if input1.frontier().is_empty() && stash.is_empty() {
//             validate_trace = None;
//         }

//     }).as_collection()
// }