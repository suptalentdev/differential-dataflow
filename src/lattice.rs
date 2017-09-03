//! Partially ordered elements with a least upper bound.
//!
//! Lattices form the basis of differential dataflow's efficient execution in the presence of 
//! iterative sub-computations. All logical times in differential dataflow must implement the 
//! `Lattice` trait, and all reasoning in operators are done it terms of `Lattice` methods.

use timely::order::PartialOrder;

/// A bounded partially ordered type supporting joins and meets.
pub trait Lattice : PartialOrder {

    /// The smallest element of the type.
    ///
    /// #Examples
    ///
    /// ```
    /// use differential_dataflow::lattice::Lattice;
    ///
    /// let min = <usize as Lattice>::minimum();
    /// assert_eq!(min, usize::min_value());
    /// ```
    fn minimum() -> Self;

    /// The largest element of the type.
    ///
    /// #Examples
    ///
    /// ```
    /// use differential_dataflow::lattice::Lattice;
    ///
    /// let max = <usize as Lattice>::maximum();
    /// assert_eq!(max, usize::max_value());
    /// ```
    fn maximum() -> Self;

    /// The smallest element greater than or equal to both arguments.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate timely;
    /// # extern crate differential_dataflow;
    /// # use timely::PartialOrder;
    /// # use timely::progress::nested::product::Product;
    /// # use differential_dataflow::lattice::Lattice;
    /// # fn main() {
    ///
    /// let time1 = Product::new(3, 7);
    /// let time2 = Product::new(4, 6);
    /// let join = time1.join(&time2);
    ///
    /// assert_eq!(join, Product::new(4, 7));
    /// # }
    /// ```
    fn join(&self, &Self) -> Self;

    /// The largest element less than or equal to both arguments.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate timely;
    /// # extern crate differential_dataflow;
    /// # use timely::PartialOrder;
    /// # use timely::progress::nested::product::Product;
    /// # use differential_dataflow::lattice::Lattice;
    /// # fn main() {
    ///
    /// let time1 = Product::new(3, 7);
    /// let time2 = Product::new(4, 6);
    /// let meet = time1.meet(&time2);
    ///
    /// assert_eq!(meet, Product::new(3, 6));
    /// # }
    /// ```    
    fn meet(&self, &Self) -> Self;

    /// Advances self to the largest time indistinguishable under `frontier`.
    ///
    /// This method produces the "largest" lattice element with the property that for every
    /// lattice element greater than some element of `frontier`, both the result and `self`
    /// compare identically to the lattice element. The result is the "largest" element in
    /// the sense that any other element with the same property (compares identically to times
    /// greater or equal to `frontier`) must be less or equal to the result.
    ///
    /// When provided an empty frontier, the result is `<Self as Lattice>::maximum()`. It should
    /// perhaps be distinguished by an `Option<Self>` type, but the `None` case only happens
    /// when `frontier` is empty, which the caller can see for themselves if they want to be
    /// clever.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate timely;
    /// # extern crate differential_dataflow;
    /// # use timely::PartialOrder;
    /// # use timely::progress::nested::product::Product;
    /// # use differential_dataflow::lattice::Lattice;
    /// # fn main() {
    ///
    /// let time = Product::new(3, 7);
    /// let frontier = vec![Product::new(4, 8), Product::new(5, 3)];
    /// let advanced = time.advance_by(&frontier[..]);
    ///
    /// // `time` and `advanced` are indistinguishable to elements >= an element of `frontier`
    /// for i in 0 .. 10 {
    ///     for j in 0 .. 10 {
    ///         let test = Product::new(i, j);
    ///         // for `test` in the future of `frontier` ..
    ///         if frontier.iter().any(|t| t.less_equal(&test)) {
    ///             assert_eq!(time.less_equal(&test), advanced.less_equal(&test));
    ///         }
    ///     }
    /// }
    ///
    /// assert_eq!(advanced, Product::new(4, 7));
    /// # }
    /// ```
    #[inline(always)]
    fn advance_by(&self, frontier: &[Self]) -> Self where Self: Sized{
        if frontier.len() > 0 {
            let mut result = self.join(&frontier[0]);
            for f in &frontier[1..] {
                result = result.meet(&self.join(f));
            }
            result
        }  
        else {
            Self::maximum()
        }
    }
}

// /// A carrier trait for totally ordered lattices.
// ///
// /// Types that implement `TotalOrder` are stating that their `Lattice` is in fact a total order.
// /// This type is only used to restrict implementations to certain types of lattices.
// ///
// /// This trait is automatically implemented for integer scalars, and for products of these types
// /// with "empty" timestamps (e.g. `RootTimestamp` and `()`). Be careful implementing this trait
// /// for your own timestamp types, as it may lead to the applicability of incorrect implementations.
// ///
// /// Note that this trait is distinct from `Ord`; many implementors of `Lattice` also implement 
// /// `Ord` so that they may be sorted, deduplicated, etc. This implementation neither derives any
// /// information from an `Ord` implementation nor informs it in any way.
// ///
// /// #Examples
// ///
// /// ```
// /// use differential_dataflow::lattice::TotalOrder;
// ///
// /// // The `join` and `meet` of totally ordered elements are always one of the two.
// /// fn invariant<T: TotalOrder>(elt1: T, elt2: T) {
// ///     if elt1.less_equal(&elt2) {
// ///         assert!(elt1.meet(&elt2) == elt1);
// ///         assert!(elt1.join(&elt2) == elt2);
// ///     }
// ///     else {
// ///         assert!(elt1.meet(&elt2) == elt2);
// ///         assert!(elt1.join(&elt2) == elt1);
// ///     }
// /// }
// /// ```
// pub trait TotalOrder : Lattice { }

use timely::progress::nested::product::Product;

impl<T1: Lattice, T2: Lattice> Lattice for Product<T1, T2> {
    #[inline(always)]
    fn minimum() -> Self { Product::new(T1::minimum(), T2::minimum()) }
    #[inline(always)]
    fn maximum() -> Self { Product::new(T1::maximum(), T2::maximum()) }
    #[inline(always)]
    fn join(&self, other: &Product<T1, T2>) -> Product<T1, T2> {
        Product {
            outer: self.outer.join(&other.outer),
            inner: self.inner.join(&other.inner),
        }
    }
    #[inline(always)]
    fn meet(&self, other: &Product<T1, T2>) -> Product<T1, T2> {
        Product {
            outer: self.outer.meet(&other.outer),
            inner: self.inner.meet(&other.inner),
        }
    }
}

macro_rules! implement_lattice {
    ($index_type:ty, $minimum:expr, $maximum:expr) => (
        impl Lattice for $index_type {
            #[inline(always)] fn minimum() -> Self { $minimum }
            #[inline(always)] fn maximum() -> Self { $maximum }
            #[inline(always)] fn join(&self, other: &Self) -> Self { ::std::cmp::max(*self, *other) }
            #[inline(always)] fn meet(&self, other: &Self) -> Self { ::std::cmp::min(*self, *other) }
        }
    )
}

use timely::progress::timestamp::RootTimestamp;

implement_lattice!(RootTimestamp, RootTimestamp, RootTimestamp);
implement_lattice!(usize, usize::min_value(), usize::max_value());
implement_lattice!(u64, u64::min_value(), u64::max_value());
implement_lattice!(u32, u32::min_value(), u32::max_value());
implement_lattice!(i32, i32::min_value(), i32::max_value());
implement_lattice!((), (), ());

// impl TotalOrder for RootTimestamp { }
// impl TotalOrder for usize { }
// impl TotalOrder for u64 { }
// impl TotalOrder for u32 { }
// impl TotalOrder for i32 { }
// impl TotalOrder for () { }
