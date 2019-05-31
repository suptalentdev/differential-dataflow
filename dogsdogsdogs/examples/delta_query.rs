extern crate timely;
extern crate graph_map;
extern crate differential_dataflow;

extern crate dogsdogsdogs;

use timely::dataflow::Scope;
use timely::dataflow::operators::probe::Handle;
use differential_dataflow::input::Input;
use graph_map::GraphMMap;

use dogsdogsdogs::altneu::AltNeu;

fn main() {

    // snag a filename to use for the input graph.
    let filename = std::env::args().nth(1).unwrap();
    let batching = std::env::args().nth(2).unwrap().parse::<usize>().unwrap();
    let inspect = std::env::args().any(|x| x == "inspect");

    timely::execute_from_args(std::env::args().skip(2), move |worker| {

        let timer = std::time::Instant::now();
        let graph = GraphMMap::new(&filename);

        let peers = worker.peers();
        let index = worker.index();

        let mut probe = Handle::new();

        let mut input = worker.dataflow::<usize,_,_>(|scope| {

            let (edges_input, edges) = scope.new_collection();

            // Graph oriented both ways, indexed by key.
            use differential_dataflow::operators::arrange::ArrangeByKey;
            let forward_key = edges.arrange_by_key();
            let reverse_key = edges.map(|(x,y)| (y,x))
                                   .arrange_by_key();

            // Graph oriented both ways, indexed by (key, val).
            use differential_dataflow::operators::arrange::ArrangeBySelf;
            let forward_self = edges.arrange_by_self();
            let reverse_self = edges.map(|(x,y)| (y,x))
                                    .arrange_by_self();

            // // Graph oriented both ways, counts of distinct vals for each key.
            // // Not required without worst-case-optimal join strategy.
            // let forward_count = edges.map(|(x,y)| x).arrange_by_self();
            // let reverse_count = edges.map(|(x,y)| y).arrange_by_self();

            // Q(a,b,c) :=  E1(a,b),  E2(b,c),  E3(a,c)
            let triangles = scope.scoped::<AltNeu<usize>,_,_>("DeltaQuery (Triangles)", |inner| {

                // Grab the stream of changes.
                let changes = edges.enter(inner);

                // Each relation we'll need.
                let forward_key_alt = forward_key.enter_at(inner, |_,_,t| AltNeu::alt(t.clone()));
                let reverse_key_alt = reverse_key.enter_at(inner, |_,_,t| AltNeu::alt(t.clone()));
                let forward_key_neu = forward_key.enter_at(inner, |_,_,t| AltNeu::neu(t.clone()));
                // let reverse_key_neu = reverse_key.enter_at(inner, |_,_,t| AltNeu::neu(t.clone()));

                // let forward_self_alt = forward_self.enter_at(inner, |_,_,t| AltNeu::alt(t.clone()));
                let reverse_self_alt = reverse_self.enter_at(inner, |_,_,t| AltNeu::alt(t.clone()));
                let forward_self_neu = forward_self.enter_at(inner, |_,_,t| AltNeu::neu(t.clone()));
                let reverse_self_neu = reverse_self.enter_at(inner, |_,_,t| AltNeu::neu(t.clone()));

                // For each relation, we form a delta query driven by changes to that relation.
                //
                // The sequence of joined relations are such that we only introduce relations
                // which share some bound attributes with the current stream of deltas.
                // Each joined relation is delayed { alt -> neu } if its position in the
                // sequence is greater than the delta stream.
                // Each joined relation is directed { forward, reverse } by whether the
                // bound variable occurs in the first or second position.

                use std::rc::Rc;
                let key1 = Rc::new(|x: &(u32, u32)| x.0);
                let key2 = Rc::new(|x: &(u32, u32)| x.1);

                use dogsdogsdogs::operators::propose;
                use dogsdogsdogs::operators::validate;

                //   dQ/dE1 := dE1(a,b), E2(b,c), E3(a,c)
                let changes1 = propose(&changes, forward_key_neu.clone(), key2.clone());
                let changes1 = validate(&changes1, forward_self_neu.clone(), key1.clone());
                let changes1 = changes1.map(|((a,b),c)| (a,b,c));

                //   dQ/dE2 := dE2(b,c), E1(a,b), E3(a,c)
                let changes2 = propose(&changes, reverse_key_alt, key1.clone());
                let changes2 = validate(&changes2, reverse_self_neu, key2.clone());
                let changes2 = changes2.map(|((b,c),a)| (a,b,c));

                //   dQ/dE3 := dE3(a,c), E1(a,b), E2(b,c)
                let changes3 = propose(&changes, forward_key_alt, key1.clone());
                let changes3 = validate(&changes3, reverse_self_alt, key2.clone());
                let changes3 = changes3.map(|((a,c),b)| (a,b,c));

                changes1.concat(&changes2).concat(&changes3).leave()
            });

            triangles
                .filter(move |_| inspect)
                .inspect(|x| println!("\tTriangle: {:?}", x))
                .probe_with(&mut probe);

            edges_input
        });

        let mut index = index;
        while index < graph.nodes() {
            input.advance_to(index);
            for &edge in graph.edges(index) {
                input.insert((index as u32, edge));
            }
            index += peers;
            input.advance_to(index);
            input.flush();
            if (index / peers) % batching == 0 {
                while probe.less_than(input.time()) {
                    worker.step();
                }
                println!("{:?}\tRound {} complete", timer.elapsed(), index);
            }
        }

    }).unwrap();
}