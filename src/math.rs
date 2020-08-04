#![allow(dead_code)]

use std::ops::{Add, Div, Mul, Sub};

macro_rules! implement_vector{
    (operator, $name:ident, $op:ident, $func:ident, $($field:ident),*) => {
        impl<T: $op<Output = T>> $op for $name<T> {
            type Output = Self;

            fn $func(self, other: Self) -> Self::Output {
                $name {
                    $($field: self.$field.$func(other.$field),)*
                }
            }
        }

        impl<T: $op<Output = T> + Clone> $op<T> for $name<T> {
            type Output = Self;

            fn $func(self, other: T) -> Self::Output {
                $name {
                    $($field: self.$field.$func(other.clone()),)*
                }
            }
        }
    };
    ($name:ident, $short_name: ident, $($field:ident),*) => {
        #[repr(C)]
        #[derive(Debug, Copy, Clone, PartialEq)]
        pub struct $name<T> {
            $(pub $field: T,)*
        }

        pub fn $short_name<T>($($field: T,)*) -> $name<T> {
            $name {
                $($field,)*
            }
        }

        impl<T: Clone> $name<T> {
            pub fn fill(v: T) -> Self {
                $name {
                    $($field: v.clone(),)*
                }
            }
        }

        impl<T> $name<T> {
            pub fn new($($field: T,)*) -> Self {
                $name {
                    $($field,)*
                }
            }
        }

        impl $name<u32> {
            pub fn as_f32(&self) -> $name<f32> {
                $name {
                    $($field: self.$field as f32,)*
                }
            }
            pub fn as_f64(&self) -> $name<f64> {
                $name {
                    $($field: self.$field as f64,)*
                }
            }
        }

        impl $name<i32> {
            pub fn as_f32(&self) -> $name<f32> {
                $name {
                    $($field: self.$field as f32,)*
                }
            }
            pub fn as_f64(&self) -> $name<f64> {
                $name {
                    $($field: self.$field as f64,)*
                }
            }
        }

        impl $name<f64> {
            pub fn distance(&self, other: &Self) -> f64 {
                (
                    $((self.$field - other.$field).powi(2) +)* 0.0
                ).sqrt()
            }

            pub fn magnitude(&self) -> f64 {
                (
                    $(self.$field.powi(2) +)* 0.0
                ).sqrt().abs()
            }

            pub fn normalize(&self) -> $name<f64> {
                let mag = self.magnitude();
                if mag > 0.0 {
                    *self / mag
                } else {
                    *self
                }
            }

            pub fn as_f32(&self) -> $name<f32> {
                $name {
                    $($field: self.$field as f32,)*
                }
            }
        }

        impl $name<f32> {
            pub fn distance(&self, other: &Self) -> f32 {
                (
                    $((self.$field - other.$field).powi(2) +)* 0.0
                ).sqrt()
            }

            pub fn magnitude(&self) -> f32 {
                (
                    $(self.$field.powi(2) +)* 0.0
                ).sqrt().abs()
            }

            pub fn normalize(&self) -> $name<f32> {
                let mag = self.magnitude();
                if mag > 0.0 {
                    *self / mag
                } else {
                    *self
                }
            }

            pub fn as_f64(&self) -> $name<f64> {
                $name {
                    $($field: self.$field as f64,)*
                }
            }
        }

        impl<T> $name<T>
        where
            T: Mul<Output = T> + Add<Output = T> + Clone + Num,
        {
            fn dot(self, other: Self) -> T {
                $(self.$field * other.$field +)* T::ZERO
            }
        }

        implement_vector!(operator, $name, Add, add, $($field),*);
        implement_vector!(operator, $name, Sub, sub, $($field),*);
        implement_vector!(operator, $name, Mul, mul, $($field),*);
        implement_vector!(operator, $name, Div, div, $($field),*);
    }
}

pub trait Num {
    const ZERO: Self;
    const ONE: Self;
    fn min(&self, other: Self) -> Self;
    fn max(&self, other: Self) -> Self;
}

impl Num for i32 {
    const ZERO: i32 = 0;
    const ONE: i32 = 1;

    fn min(&self, other: Self) -> Self {
        std::cmp::Ord::min(*self, other)
    }
    fn max(&self, other: Self) -> Self {
        std::cmp::Ord::max(*self, other)
    }
}

impl Num for u32 {
    const ZERO: u32 = 0;
    const ONE: u32 = 1;

    fn min(&self, other: Self) -> Self {
        std::cmp::Ord::min(*self, other)
    }
    fn max(&self, other: Self) -> Self {
        std::cmp::Ord::max(*self, other)
    }
}

impl Num for f32 {
    const ZERO: f32 = 0.0;
    const ONE: f32 = 1.0;

    fn min(&self, other: Self) -> Self {
        f32::min(*self, other)
    }
    fn max(&self, other: Self) -> Self {
        f32::max(*self, other)
    }
}

impl Num for f64 {
    const ZERO: f64 = 0.0;
    const ONE: f64 = 1.0;

    fn min(&self, other: Self) -> Self {
        f64::min(*self, other)
    }
    fn max(&self, other: Self) -> Self {
        f64::max(*self, other)
    }
}

implement_vector!(V2, v2, x, y);
implement_vector!(V3, v3, x, y, z);
implement_vector!(V4, v4, x, y, z, w);

impl<T> V2<T> {
    pub fn expand(self, z: T) -> V3<T> {
        V3::new(self.x, self.y, z)
    }
}

impl<T> V3<T> {
    pub fn expand(self, w: T) -> V4<T> {
        V4::new(self.x, self.y, self.z, w)
    }

    pub fn contract(self) -> V2<T> {
        V2::new(self.x, self.y)
    }
}

impl<T> V3<T>
where
    T: Div<Output = T> + Clone,
{
    pub fn collapse(self) -> V2<T> {
        V2::new(self.x / self.z.clone(), self.y / self.z)
    }
}

impl<T> V4<T> {
    pub fn contract(self) -> V3<T> {
        V3::new(self.x, self.y, self.z)
    }
}

impl<T> V4<T>
where
    T: Div<Output = T> + Clone,
{
    pub fn collapse(self) -> V3<T> {
        V3::new(
            self.x / self.w.clone(),
            self.y / self.w.clone(),
            self.z / self.w.clone(),
        )
    }
}

#[repr(C)]
#[derive(Debug, Copy, PartialEq)]
pub struct M3<T> {
    pub c0: V3<T>,
    pub c1: V3<T>,
    pub c2: V3<T>,
}

fn m3<T>(c0: V3<T>, c1: V3<T>, c2: V3<T>) -> M3<T> {
    M3::new(c0, c1, c2)
}

impl<T> M3<T> {
    pub fn new(c0: V3<T>, c1: V3<T>, c2: V3<T>) -> M3<T> {
        M3 { c0, c1, c2 }
    }

    pub fn transpose(self) -> M3<T> {
        M3 {
            c0: V3::new(self.c0.x, self.c1.x, self.c2.x),
            c1: V3::new(self.c0.y, self.c1.y, self.c2.y),
            c2: V3::new(self.c0.z, self.c1.z, self.c2.z),
        }
    }
}

impl<T: Clone> Clone for M3<T> {
    fn clone(&self) -> Self {
        Self::new(self.c0.clone(), self.c1.clone(), self.c2.clone())
    }
}

impl<T: Num> M3<T> {
    pub fn identity() -> Self {
        Self::new(
            V3::new(T::ONE, T::ZERO, T::ZERO),
            V3::new(T::ZERO, T::ONE, T::ZERO),
            V3::new(T::ZERO, T::ZERO, T::ONE),
        )
    }
}

impl<T> Mul<M3<T>> for M3<T>
where
    T: Mul<Output = T> + Add<Output = T> + Num + Clone,
{
    type Output = M3<T>;

    fn mul(self, rhs: M3<T>) -> Self::Output {
        let m = self.transpose();

        let c00 = m.c0.clone().dot(rhs.c0.clone());
        let c01 = m.c1.clone().dot(rhs.c0.clone());
        let c02 = m.c2.clone().dot(rhs.c0);

        let c10 = m.c0.clone().dot(rhs.c1.clone());
        let c11 = m.c1.clone().dot(rhs.c1.clone());
        let c12 = m.c2.clone().dot(rhs.c1);

        let c20 = m.c0.dot(rhs.c2.clone());
        let c21 = m.c1.dot(rhs.c2.clone());
        let c22 = m.c2.dot(rhs.c2);

        M3::new(
            V3::new(c00, c01, c02),
            V3::new(c10, c11, c12),
            V3::new(c20, c21, c22),
        )
    }
}

impl<T> Mul<V3<T>> for M3<T>
where
    T: Mul<Output = T> + Add<Output = T> + Clone,
{
    type Output = V3<T>;

    fn mul(self, rhs: V3<T>) -> Self::Output {
        let m = self;
        let vx = m.c0 * rhs.x;
        let vy = m.c1 * rhs.y;
        let vz = m.c2 * rhs.z;
        V3::new(vx.x + vy.x + vz.x, vx.y + vy.y + vz.y, vx.z + vy.z + vz.z)
    }
}

#[repr(C)]
#[derive(Debug, Copy, PartialEq)]
pub struct M4<T> {
    pub c0: V4<T>,
    pub c1: V4<T>,
    pub c2: V4<T>,
    pub c3: V4<T>,
}

fn m4<T>(c0: V4<T>, c1: V4<T>, c2: V4<T>, c3: V4<T>) -> M4<T> {
    M4::new(c0, c1, c2, c3)
}

impl<T> M4<T> {
    pub fn new(c0: V4<T>, c1: V4<T>, c2: V4<T>, c3: V4<T>) -> M4<T> {
        M4 { c0, c1, c2, c3 }
    }

    pub fn transpose(self) -> M4<T> {
        M4 {
            c0: V4::new(self.c0.x, self.c1.x, self.c2.x, self.c3.x),
            c1: V4::new(self.c0.y, self.c1.y, self.c2.y, self.c3.y),
            c2: V4::new(self.c0.z, self.c1.z, self.c2.z, self.c3.z),
            c3: V4::new(self.c0.w, self.c1.w, self.c2.w, self.c3.w),
        }
    }
}

impl<T: Clone> Clone for M4<T> {
    fn clone(&self) -> Self {
        Self::new(
            self.c0.clone(),
            self.c1.clone(),
            self.c2.clone(),
            self.c3.clone(),
        )
    }
}

impl<T: Num> M4<T> {
    pub fn identity() -> Self {
        Self::new(
            V4::new(T::ONE, T::ZERO, T::ZERO, T::ZERO),
            V4::new(T::ZERO, T::ONE, T::ZERO, T::ZERO),
            V4::new(T::ZERO, T::ZERO, T::ONE, T::ZERO),
            V4::new(T::ZERO, T::ZERO, T::ZERO, T::ONE),
        )
    }
}

impl<T> Mul<M4<T>> for M4<T>
where
    T: Mul<Output = T> + Add<Output = T> + Num + Clone,
{
    type Output = M4<T>;

    fn mul(self, rhs: M4<T>) -> Self::Output {
        let m = self.transpose();

        let c00 = m.c0.clone().dot(rhs.c0.clone());
        let c01 = m.c1.clone().dot(rhs.c0.clone());
        let c02 = m.c2.clone().dot(rhs.c0.clone());
        let c03 = m.c3.clone().dot(rhs.c0.clone());

        let c10 = m.c0.clone().dot(rhs.c1.clone());
        let c11 = m.c1.clone().dot(rhs.c1.clone());
        let c12 = m.c2.clone().dot(rhs.c1.clone());
        let c13 = m.c3.clone().dot(rhs.c1.clone());

        let c20 = m.c0.clone().dot(rhs.c2.clone());
        let c21 = m.c1.clone().dot(rhs.c2.clone());
        let c22 = m.c2.clone().dot(rhs.c2.clone());
        let c23 = m.c3.clone().dot(rhs.c2.clone());

        let c30 = m.c0.dot(rhs.c3.clone());
        let c31 = m.c1.dot(rhs.c3.clone());
        let c32 = m.c2.dot(rhs.c3.clone());
        let c33 = m.c3.dot(rhs.c3.clone());

        M4::new(
            V4::new(c00, c01, c02, c03),
            V4::new(c10, c11, c12, c13),
            V4::new(c20, c21, c22, c23),
            V4::new(c30, c31, c32, c33),
        )
    }
}

impl<T> Mul<V4<T>> for M4<T>
where
    T: Mul<Output = T> + Add<Output = T> + Clone,
{
    type Output = V4<T>;

    fn mul(self, rhs: V4<T>) -> Self::Output {
        let m = self;
        let vx = m.c0 * rhs.x;
        let vy = m.c1 * rhs.y;
        let vz = m.c2 * rhs.z;
        let vw = m.c3 * rhs.w;
        V4::new(
            vx.x + vy.x + vz.x + vw.x,
            vx.y + vy.y + vz.y + vw.y,
            vx.z + vy.z + vz.z + vw.z,
            vx.w + vy.w + vz.w + vw.w,
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Rect<T> {
    pub min: V2<T>,
    pub max: V2<T>,
}

impl<T: Sub<Output = T> + Num + Clone + Copy + PartialOrd> Rect<T> {
    pub fn new(min: V2<T>, max: V2<T>) -> Self {
        Rect {
            min: V2::new(min.x.min(max.x), min.y.min(max.y)),
            max: V2::new(min.x.max(max.x), min.y.max(max.y)),
        }
    }

    pub fn width(&self) -> T {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> T {
        self.max.y - self.min.y
    }

    pub fn contains(&self, point: V2<T>) -> bool {
        self.min.x < point.x && self.max.x > point.x && self.min.y < point.y && self.max.y > point.y
    }

    pub fn triangle_list_iter(&self) -> TriangleListIter<T> {
        TriangleListIter {
            arr: [
                self.min,
                V2::new(self.min.x, self.max.y),
                V2::new(self.max.x, self.min.y),
                self.max,
                V2::new(self.max.x, self.min.y),
                V2::new(self.min.x, self.max.y),
            ],
            index: 0,
        }
    }
}

impl Rect<i32> {
    pub fn as_f32(&self) -> Rect<f32> {
        Rect::new(self.min.as_f32(), self.max.as_f32())
    }
}

pub struct TriangleListIter<T: Copy> {
    arr: [V2<T>; 6],
    index: usize,
}

impl<T: Copy> Iterator for TriangleListIter<T> {
    type Item = V2<T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= 6 {
            None
        } else {
            let ret = self.arr[self.index];
            self.index += 1;
            Some(ret)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn martix_matrix_mul() {
        let identity = M3::<f32>::identity();

        let num = m3(v3(1.0, 2.0, 3.0), v3(4.0, 5.0, 6.0), v3(7.0, 8.0, 9.0));

        let result = num.clone() * identity.clone();
        assert_eq!(result, num);

        let result = identity.clone() * num.clone();
        assert_eq!(result, num);

        let left = m3(v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 0.0), v3(0.0, 2.0, 0.0));
        let num = m3(v3(1.0, 4.0, 7.0), v3(2.0, 5.0, 8.0), v3(3.0, 6.0, 9.0));

        let result = m3(v3(1.0, 14.0, 0.0), v3(2.0, 16.0, 0.0), v3(3.0, 18.0, 0.0));

        assert_eq!(left.clone() * num.clone(), result);
        assert_ne!(num * left, result);
    }
}
