#[allow(unused_variables)]
extern crate fnv;
extern crate rand;
extern crate time;
extern crate timely;
extern crate differential_dataflow;

use std::collections::HashMap;
use std::io::{BufReader, BufRead};
use std::fs::File;

use timely::dataflow::*;
use timely::dataflow::scopes::Child;
use timely::dataflow::operators::*;
use timely::dataflow::operators::feedback::Handle;
use timely::progress::timestamp::RootTimestamp;

use differential_dataflow::Data;
use differential_dataflow::operators::*;
use differential_dataflow::collection::LeastUpperBound;
use differential_dataflow::operators::join::{Join, JoinUnsigned};

use differential_dataflow::collection::robin_hood::RHHMap;

/// A collection defined by multiple mutually recursive rules.
pub struct Variable<G: Scope, D: Default+Data>
where G::Timestamp: LeastUpperBound {
    feedback: Option<Handle<G::Timestamp, u64,(D, i32)>>,
    current:  Stream<Child<G, u64>, (D,i32)>,
}

impl<G: Scope, D: Default+Data> Variable<G, D> where G::Timestamp: LeastUpperBound {
    /// Creates a new `Variable` and a `Stream` representing its output, from a supplied `source` stream.
    pub fn from(source: &Stream<Child<G, u64>, (D,i32)>) -> (Variable<G, D>, Stream<Child<G,u64>, (D, i32)>) {
        let (feedback, cycle) = source.scope().loop_variable(u64::max_value(), 1);
        let mut result = Variable { feedback: Some(feedback), current: cycle.clone() };
        let stream = cycle.clone();
        result.add(source);
        (result, stream)
    }
    /// Adds a new source of data to the `Variable`.
    pub fn add(&mut self, source: &Stream<Child<G, u64>, (D,i32)>) {
        self.current = self.current.concat(source);
    }
}

impl<G: Scope, D: Default+Data> Drop for Variable<G, D> where G::Timestamp: LeastUpperBound {
    fn drop(&mut self) {
        if let Some(feedback) = self.feedback.take() {
            self.current.threshold(|x| x.hashed(), 
                // |_| HashMap::new(), 
                |_| RHHMap::new(|x: &D| x.hashed() as usize),
                |_, w| if w > 0 { 1 } else { 0 })
                        .connect_loop(feedback);
        }
    }
}

/// A collection defined by multiple mutually recursive rules.
pub struct NewVariable<G: Scope, D: Default+Data>
where G::Timestamp: LeastUpperBound {
    feedback: Option<Handle<G::Timestamp, u64,(D, i32)>>,
    source:  Stream<Child<G, u64>, (D,i32)>,
}

impl<G: Scope, D: Default+Data> NewVariable<G, D> where G::Timestamp: LeastUpperBound {
    /// Creates a new `Variable` and a `Stream` representing its output, from a supplied `source` stream.
    pub fn from(source: &Stream<Child<G, u64>, (D,i32)>) -> (NewVariable<G, D>, Stream<Child<G,u64>, (D, i32)>) {
        let (feedback, cycle) = source.scope().loop_variable(u64::max_value(), 1);
        let result = NewVariable { feedback: Some(feedback), source: cycle.clone() };
        let stream = cycle.clone();
        (result, stream)
    }
    /// Adds a new source of data to the `Variable`.
    pub fn set(mut self, result: &Stream<Child<G, u64>, (D,i32)>) {
        if let Some(feedback) = self.feedback.take() {
            self.source.map_in_place(|x| x.1 *= -1)
                       .concat(result)
                       .connect_loop(feedback);
        }
    }
}

impl<G: Scope, D: Default+Data> Drop for NewVariable<G, D> where G::Timestamp: LeastUpperBound {
    fn drop(&mut self) {
        if self.feedback.is_some() {
            panic!("unset new_variable");
        }
    }
}

macro_rules! rule {
    ($name1: ident ($($var1:ident),*) := $name2: ident ($($var2:ident),*) $name3: ident ($($var3:ident),*) : ($($var4:ident),*) = ($($var5:ident),*)) => {{
        let result =
            $name2.0.join_by(
                &$name3.0,
                |($( $var2, )*)| (($( $var4, )*), ( $($var2, )*)),
                |($( $var3, )*)| (($( $var5, )*), ( $($var3, )*)),
                |x| x.hashed(),
                |_, &($( $var2, )*), &($( $var3, )*)| (($( $var2, )*), ($( $var3, )*)));
        $name1.1.add(&result.map(|((($( $var2, )*), ($( $var3, )*)), __w)| (($( $var1, )*), __w)));

        let temp = result.filter(|_| false).semijoin_by(
            &$name1.2,
            |(($( $var2, )*), ($( $var3, )*))| (($( $var1, )*), (($( $var2, )*), ($( $var3, )*))),
            |x| x.hashed(),
            |_, &(($( $var2, )*), ($( $var3, )*))| (($( $var2, )*), ($( $var3, )*)));
        $name2.3.add(&temp.map(|(( ($( $var2, )*) ,_),__w)| (($( $var2, )*),__w)));
        $name3.3.add(&temp.map(|(( _, ($( $var3, )*)),__w)| (($( $var3, )*),__w)));

        temp
    }};
}

macro_rules! rule_3 {
    ($name1: ident ($($var1:ident),*) := $name2: ident ($($var2:ident),*) $name3: ident ($($var3:ident),*) $name4: ident ($($var4:ident),*) : $key1:ident = $key2:ident, ($($key3:ident),*) = ($($key4:ident),*)) => {{
        let result =
            $name2.0.join_by_u(
                &$name3.0,
                |($( $var2, )*)| ($key1, ( $($var2, )*)),
                |($( $var3, )*)| ($key2, ( $($var3, )*)),
                // |x| x.hashed(),
                |_, &($( $var2, )*), &($( $var3, )*)| (($( $var2, )*), ($( $var3, )*)));

        // result.map(|(_,w)| ((),w)).consolidate_by(|_| 0u64).inspect_batch(|t,b| { let mut sum = 0i64; for &(_,w) in b.iter() { sum += w as i64; } println!("intermediate: {} @ {:?}", sum, t); });

        let result = result.join_by(
                &$name4.0,
                |(($( $var2, )*), ($( $var3, )*))| (($( $key3, )*), (($( $var2, )*), ($( $var3, )*))),
                |($( $var4, )*)| (($( $key4, )*), ( $($var4, )*)),
                |x| x.hashed(),
                |_, &(($( $var2, )*), ($( $var3, )*)), &($( $var4, )*)| (($( $var2, )*), ($( $var3, )*), ($( $var4, )*))
            );

        $name1.1.add(&result.map(|((($( $var2, )*), ($( $var3, )*), ($( $var4, )*)), __w)| (($( $var1, )*), __w)));

        let temp = result.filter(|_| false).semijoin_by(
            &$name1.2,
            |(($( $var2, )*), ($( $var3, )*), ($( $var4, )*))| (($( $var1, )*), (($( $var2, )*), ($( $var3, )*), ($( $var4, )*))),
            |x| x.hashed(),
            |_, &(($( $var2, )*), ($( $var3, )*), ($( $var4, )*))| (($( $var2, )*), ($( $var3, )*), ($( $var4, )*)));
        $name2.3.add(&temp.map(|((($( $var2, )*),_,_),__w)| (($( $var2, )*),__w)));
        $name3.3.add(&temp.map(|((_,($( $var3, )*),_),__w)| (($( $var3, )*),__w)));
        $name4.3.add(&temp.map(|((_,_,($( $var4, )*)),__w)| (($( $var4, )*),__w)));

        temp
    }};
}

macro_rules! rule_u {
    ($name1: ident ($($var1:ident),*) := $name2: ident ($($var2:ident),*) $name3: ident ($($var3:ident),*) : $var4:ident = $var5:ident) => {{
        let result =
            $name2.0.join_by_u(
                &$name3.0,
                |($( $var2, )*)| ($var4, ( $($var2, )*)),
                |($( $var3, )*)| ($var5, ( $($var3, )*)),
                |_, &($( $var2, )*), &($( $var3, )*)| (($( $var2, )*), ($( $var3, )*)));
        $name1.1.add(&result.map(|((($( $var2, )*), ($( $var3, )*)), __w)| (($( $var1, )*), __w)));

        let temp = result.filter(|_| false).semijoin_by(
            &$name1.2,
            |(($( $var2, )*), ($( $var3, )*))| (($( $var1, )*), (($( $var2, )*), ($( $var3, )*))),
            |x| x.hashed(),
            |_, &(($( $var2, )*), ($( $var3, )*))| (($( $var2, )*), ($( $var3, )*)));
        $name2.3.add(&temp.map(|(( ($( $var2, )*) ,_),__w)| (($( $var2, )*),__w)));
        $name3.3.add(&temp.map(|(( _, ($( $var3, )*)),__w)| (($( $var3, )*),__w)));

        temp
    }};
}

macro_rules! variable {
    ($name0: ident : $name1: expr, $name2: expr) => {{
        let temp1 = Variable::from(&$name0.enter(&$name1));
        let temp2 = Variable::from(&$name0.enter(&$name2));
        (temp1.1, temp1.0, temp2.1, temp2.0)
    }}
}

fn for_each_line<F: FnMut(String)>(filename: &str, mut logic: F) {
    let file = BufReader::new(File::open(filename).unwrap());
    for readline in file.lines() {
        let line = readline.ok().expect("read error");
        logic(line);
    }
}

fn for_each_pair_in<F: FnMut((u32, u32))>(filename: &str, mut logic: F) {
    for_each_line(filename, |string| {
        let mut fields = string[..].split(",");
        let a: u32 = fields.next().unwrap().parse().ok().expect("parse error");
        let b: u32 = fields.next().unwrap().parse().ok().expect("parse error");
        logic((a, b))
    });
}

fn for_each_trip_in<F: FnMut((u32, u32, u32))>(filename: &str, mut logic: F) {
    for_each_line(filename, |string| {
        let mut fields = string[..].split(",");
        let a: u32 = fields.next().unwrap().parse().ok().expect("parse error");
        let b: u32 = fields.next().unwrap().parse().ok().expect("parse error");
        let c: u32 = fields.next().unwrap().parse().ok().expect("parse error");
        logic((a, b, c))
    });
}


fn main() {

    timely::execute_from_args(std::env::args(), |root| {

        let start = time::precise_time_s();
        let (mut c, mut p, mut q, mut r, mut s, mut u, mut p_query, mut q_query, probe) = root.scoped::<u64, _, _>(move |outer| {

            // inputs for p, q, and u base facts.
            let (c_input, c) = outer.new_input();
            let (p_input, p) = outer.new_input();
            let (q_input, q) = outer.new_input();
            let (r_input, r) = outer.new_input();
            let (s_input, s) = outer.new_input();
            let (u_input, u) = outer.new_input();

            // inputs through which to demand explanations.
            let (_c_query_input, c_query) = outer.new_input();
            let (p_query_input, p_query) = outer.new_input();
            let (q_query_input, q_query) = outer.new_input();
            let (_r_query_input, r_query) = outer.new_input();
            let (_s_query_input, s_query) = outer.new_input();
            let (_u_query_input, u_query) = outer.new_input();

            // derive each of the firings of each of the rules, as well as tuples deleted from
            // both p and q to ensure the emptiness of
            let (p_del, _q_del, _ir1, _ir2, _ir3, _ir4, _ir5, _ir6) = outer.scoped::<u64,_,_>(|middle| {

                // an evolving set of things we may want to remove from p_edb and q_edb.
                let mut p_del = Variable::from(&middle.enter(&p).filter(|_| false));
                let mut q_del = Variable::from(&middle.enter(&q).filter(|_| false));

                // bring outer streams into the middle scope
                let (c_edb, c_query) = (middle.enter(&c), middle.enter(&c_query));
                let (p_edb, p_query) = (middle.enter(&p).concat(&p_del.1.map(|(x,w)|(x,-w))).consolidate(), middle.enter(&p_query));
                let (q_edb, q_query) = (middle.enter(&q).concat(&q_del.1.map(|(x,w)|(x,-w))).consolidate(), middle.enter(&q_query));
                let (r_edb, r_query) = (middle.enter(&r), middle.enter(&r_query));
                let (s_edb, s_query) = (middle.enter(&s), middle.enter(&s_query));
                let (u_edb, u_query) = (middle.enter(&u), middle.enter(&u_query));

                // determine derived p and q tuples, instances of p and q tuples which participate
                // in the derivation of query (secret) tuples, and also which rules fire with what
                // variable settings.
                let (_p_der, _q_der, p_bad, q_bad, ir1, ir2, ir3, ir4, ir5, ir6) = middle.scoped::<u64, _, _>(|inner| {

                    // track derived P and Q tuples, and any that need explanation.
                    let mut c = variable!(inner : c_edb, c_query);
                    let mut p = variable!(inner : p_edb, p_query);
                    let mut q = variable!(inner : q_edb, q_query);
                    let mut r = variable!(inner : r_edb, r_query);
                    let mut s = variable!(inner : s_edb, s_query);
                    let mut u = variable!(inner : u_edb, u_query);

                    let ir1 = rule_u!(p(x,z)   := p(x,_y1) p(_y2,z)   : _y1 = _y2);
                    let ir2 = rule_u!(q(x,r,z) := p(x,_y1) q(_y2,r,z) : _y1 = _y2);
                    let ir3 = rule_3!(p(x,z)   := p(_y1,_w1) u(_w2,_r2,z) q(x,_r1,_y2)  : _w1 = _w2, (_y1,_r2) = (_y2,_r1));
                    // let ir4 = rule_3!(p(_x1,z) := p(_x1,_y1) p(_x2,_w1) c(_y2,_w2,z) : (_x1) = (_x2), (_y1,_w1) = (_y2,_w2));
                    let ir4 = rule_3!(p(_x1,z) := c(_y2,_w2,z) p(_x2,_w1) p(_x1,_y1) : _w2 = _w1, (_y2,_x2) = (_y1,_x1));
                    let ir5 = rule_u!(q(x,q,z) := q(x,_r1,z) s(_r2,q) : _r1 = _r2);
                    let ir6 = rule_3!(q(x,e,o) := q(x,_y1,_z1) r(_y2,_u2,e) q(_z2,_u1,o) : _y1 = _y2, (_z1,_u2) = (_z2,_u1));

                    // extract the results and return
                    (p.0.leave(), q.0.leave(), p.2.leave(), q.2.leave(), 
                        ir1.leave(), ir2.leave(), ir3.leave(), ir4.leave(), ir5.leave(), ir6.leave())
                });

                // _p_der.inspect(|&((x,y),_)| println!("{},{}", x, y));
                // _q_der.inspect(|&((x,y,z),_)| println!("{},{},{}", x, y,z));

                // p_bad and q_bad are p and q tuples involved in the derivation.
                // we should remove some of them from p, q by adding them to p_del, q_del.
                let p_bad = p_bad.map(|(x,w)| ((x,()),w)).join(&p_edb.map(|(x,w)| ((x,()),w))).map(|((x,(),()),w)| (((),x),w));
                let q_bad = q_bad.map(|(x,w)| ((x,()),w)).join(&q_edb.map(|(x,w)| ((x,()),w))).map(|((x,(),()),w)| (((),x),w));

                let p_bad_new = p_bad.cogroup_by_inner(&q_bad, |k| k.hashed(), |_,&x| x,
                            |_| HashMap::new(), |_key, input1, _input2, output| {
                     output.push(input1.next().map(|(&x,w)|(x,w)).unwrap());
                });

                let q_bad_new = p_bad.cogroup_by_inner(&q_bad, |k| k.hashed(), |_,&x| x,
                            |_| HashMap::new(), |_key, input1, input2, output| {
                    if input1.next() == None { input2.next().map(|(&x,w)| output.push((x,w))); }
                });

                p_del.0.add(&p_bad_new);
                q_del.0.add(&q_bad_new);

                (p_del.1.leave(), q_del.1.leave(), ir1.leave(), ir2.leave(), ir3.leave(), ir4.leave(), ir5.leave(), ir6.leave())
            });

            let (probe, _) = p_del.consolidate().probe();

            (c_input, p_input, q_input, r_input, s_input, u_input, p_query_input, q_query_input, probe)
        });

        // worker 0 loads the data
        if root.index() == 0 {
            for_each_trip_in("/Users/mcsherry/Projects/Datasets/snomed/c.txt", |x| c.send((x,1)));
            for_each_pair_in("/Users/mcsherry/Projects/Datasets/snomed/p.txt", |x| p.send((x,1)));
            for_each_trip_in("/Users/mcsherry/Projects/Datasets/snomed/q.txt", |x| q.send((x,1)));
            for_each_trip_in("/Users/mcsherry/Projects/Datasets/snomed/r.txt", |x| r.send((x,1)));
            for_each_pair_in("/Users/mcsherry/Projects/Datasets/snomed/s.txt", |x| s.send((x,1)));
            for_each_trip_in("/Users/mcsherry/Projects/Datasets/snomed/u.txt", |x| u.send((x,1)));
        }

        // println!("loading:\t{}", time::precise_time_s() - start);

        // close all of the data inputs, for now.
        // could leave them open to see perf hit.
        c.close();
        p.close();
        q.close();
        r.close();
        s.close();
        u.close();

        p_query.advance_to(1);
        q_query.advance_to(1);

        // step until clear, then once more to garbage collect.
        while probe.lt(&RootTimestamp::new(1)) { root.step(); }
        root.step();

        // println!("derivation:\t{}", time::precise_time_s() - start);
        let timer = time::precise_time_s();

        // p_query.send(((1u32,3u32), 1));
        q_query.send(((1u32,4u32,5u32), 1));
        p_query.advance_to(2);
        q_query.advance_to(2);

        while probe.lt(&RootTimestamp::new(2)) { root.step(); }

        // println!("query:\t{}", time::precise_time_s() - timer);
    });
}
