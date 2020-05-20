#![allow(dead_code)]

use std::ops::{Add, Div, Mul, Sub};

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct V2<T> {
    pub x: T,
    pub y: T,
}

pub fn v2<T>(x: T, y: T) -> V2<T> {
    V2::new(x, y)
}

impl<T> From<(T, T)> for V2<T> {
    fn from((x, y): (T, T)) -> Self {
        V2 { x, y }
    }
}

impl<T: Clone> V2<T> {
    pub fn fill(xy: T) -> V2<T> {
        V2 {
            x: xy.clone(),
            y: xy,
        }
    }
}

impl<T> V2<T> {
    pub fn new(x: T, y: T) -> V2<T> {
        V2 { x, y }
    }

    pub fn expand(self, z: T) -> V3<T> {
        V3::new(self.x, self.y, z)
    }
}

impl V2<u32> {
    pub fn as_f32(&self) -> V2<f32> {
        V2 {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

impl V2<f64> {
    pub fn distance(&self, other: &Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn magnitude(&self) -> f64 {
        (self.x.powi(2) + self.y.powi(2)).sqrt().abs()
    }

    pub fn normalize(&self) -> V2<f64> {
        let mag = self.magnitude();
        if mag > 0.0 {
            *self / mag
        } else {
            *self
        }
    }

    pub fn as_f32(&self) -> V2<f32> {
        V2 {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

impl V2<f32> {
    pub fn distance(&self, other: &Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn magnitude(&self) -> f32 {
        (self.x.powi(2) + self.y.powi(2)).sqrt().abs()
    }

    pub fn normalize(&self) -> V2<f32> {
        let mag = self.magnitude();
        if mag > 0.0 {
            *self / mag
        } else {
            *self
        }
    }

    pub fn as_f64(&self) -> V2<f64> {
        V2 {
            x: self.x as f64,
            y: self.y as f64,
        }
    }
}

impl<T: Add<Output = T> + Clone> Add for V2<T> {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        V2 {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl<T: Sub<Output = T> + Clone> Sub for V2<T> {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        V2 {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl<T: Mul<Output = T> + Clone> Mul for V2<T> {
    type Output = Self;

    fn mul(self, other: Self) -> Self::Output {
        V2 {
            x: self.x * other.x,
            y: self.y * other.y,
        }
    }
}

impl<T: Div<Output = T> + Clone> Div for V2<T> {
    type Output = Self;

    fn div(self, other: Self) -> Self::Output {
        V2 {
            x: self.x / other.x,
            y: self.y / other.y,
        }
    }
}

impl<T: Add<Output = T> + Clone> Add<T> for V2<T> {
    type Output = Self;

    fn add(self, other: T) -> Self::Output {
        V2 {
            x: self.x + other.clone(),
            y: self.y + other,
        }
    }
}

impl<T: Sub<Output = T> + Clone> Sub<T> for V2<T> {
    type Output = Self;

    fn sub(self, other: T) -> Self::Output {
        V2 {
            x: self.x - other.clone(),
            y: self.y - other,
        }
    }
}

impl<T: Mul<Output = T> + Clone> Mul<T> for V2<T> {
    type Output = Self;

    fn mul(self, other: T) -> Self::Output {
        V2 {
            x: self.x * other.clone(),
            y: self.y * other,
        }
    }
}

impl<T: Div<Output = T> + Clone> Div<T> for V2<T> {
    type Output = Self;

    fn div(self, other: T) -> Self::Output {
        V2 {
            x: self.x / other.clone(),
            y: self.y / other,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct V3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

impl<T: Clone> From<(T, T, T)> for V3<T> {
    fn from(other: (T, T, T)) -> Self {
        V3 {
            x: other.0,
            y: other.1,
            z: other.2,
        }
    }
}

impl<T: Clone> V3<T> {
    pub fn fill(xyz: T) -> V3<T> {
        V3 {
            x: xyz.clone(),
            y: xyz.clone(),
            z: xyz,
        }
    }
}

pub fn v3<T>(x: T, y: T, z: T) -> V3<T> {
    V3::new(x, y, z)
}

impl<T> V3<T> {
    pub fn new(x: T, y: T, z: T) -> V3<T> {
        V3 { x, y, z }
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

impl V3<f64> {
    pub fn distance(&self, other: &Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2))
            .sqrt()
    }

    pub fn magnitude(&self) -> f64 {
        (self.x.powi(2) + self.y.powi(2) + self.z.powi(2))
            .sqrt()
            .abs()
    }

    pub fn as_f32(&self) -> V3<f32> {
        V3 {
            x: self.x as f32,
            y: self.y as f32,
            z: self.z as f32,
        }
    }
}

impl V3<f32> {
    pub fn distance(&self, other: &Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2))
            .sqrt()
    }

    pub fn magnitude(&self) -> f32 {
        (self.x.powi(2) + self.y.powi(2) + self.z.powi(2))
            .sqrt()
            .abs()
    }

    pub fn as_f64(&self) -> V3<f64> {
        V3 {
            x: self.x as f64,
            y: self.y as f64,
            z: self.z as f64,
        }
    }
}

impl<T: Add<Output = T> + Clone> Add for V3<T> {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        V3 {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }
}

impl<T: Sub<Output = T> + Clone> Sub for V3<T> {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        V3 {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
}

impl<T: Mul<Output = T> + Clone> Mul for V3<T> {
    type Output = Self;

    fn mul(self, other: Self) -> Self::Output {
        V3 {
            x: self.x * other.x,
            y: self.y * other.y,
            z: self.z * other.z,
        }
    }
}

impl<T: Div<Output = T> + Clone> Div for V3<T> {
    type Output = Self;

    fn div(self, other: Self) -> Self::Output {
        V3 {
            x: self.x / other.x,
            y: self.y / other.y,
            z: self.z / other.z,
        }
    }
}

impl<T: Mul<Output = T> + Clone> Mul<T> for V3<T> {
    type Output = Self;

    fn mul(self, other: T) -> Self::Output {
        V3 {
            x: self.x * other.clone(),
            y: self.y * other.clone(),
            z: self.z * other,
        }
    }
}

impl<T: Div<Output = T> + Clone> Div<T> for V3<T> {
    type Output = Self;

    fn div(self, other: T) -> Self::Output {
        V3 {
            x: self.x / other.clone(),
            y: self.y / other.clone(),
            z: self.z / other,
        }
    }
}

impl<T> V3<T>
where
    T: Mul<Output = T> + Add<Output = T> + Clone,
{
    fn dot(self, other: V3<T>) -> T {
        (self.x * other.x) + (self.y * other.y) + (self.z * other.z)
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

impl M3<f32> {
    pub fn identity() -> Self {
        Self::new(
            V3::new(1.0, 0.0, 0.0),
            V3::new(0.0, 1.0, 0.0),
            V3::new(0.0, 0.0, 1.0),
        )
    }
}

impl M3<f64> {
    pub fn identity() -> Self {
        Self::new(
            V3::new(1.0, 0.0, 0.0),
            V3::new(0.0, 1.0, 0.0),
            V3::new(0.0, 0.0, 1.0),
        )
    }
}

impl<T> Mul<M3<T>> for M3<T>
where
    T: Mul<Output = T> + Add<Output = T> + Clone,
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
