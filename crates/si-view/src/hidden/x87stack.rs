// SPDX-License-Identifier: MIT
//! A model of the x87 register stack for the routines of `hidden.c` whose compiler-chosen
//! order of operations (which products stay in registers, which are spilled to doubles)
//! cannot be written as an expression. Each method is one instruction of the original.

use si_x87::F80;

/// The register stack; `st(0)` is the last element.
#[derive(Default)]
pub struct X(Vec<F80>);

impl X {
    pub fn push(&mut self, v: f64) {
        self.0.push(F80::from_f64(v));
    }
    fn at(&self, i: usize) -> usize {
        self.0.len() - 1 - i
    }
    pub fn st0(&self) -> F80 {
        *self.0.last().unwrap()
    }
    pub fn push80(&mut self, v: F80) {
        self.0.push(v);
    }
    pub fn pop(&mut self) {
        self.0.pop();
    }
    pub fn ld_st(&mut self, i: usize) {
        let v = self.0[self.at(i)];
        self.0.push(v);
    }
    pub fn xch(&mut self, i: usize) {
        let (a, b) = (self.at(0), self.at(i));
        self.0.swap(a, b);
    }
    fn top(&mut self) -> &mut F80 {
        self.0.last_mut().unwrap()
    }
    pub fn fmul_st(&mut self, i: usize) {
        let v = self.0[self.at(i)];
        *self.top() = self.st0() * v;
    }
    pub fn fmul_to(&mut self, i: usize) {
        let k = self.at(i);
        self.0[k] = self.0[k] * self.st0();
    }
    pub fn fmul_m(&mut self, v: f64) {
        *self.top() = self.st0() * F80::from_f64(v);
    }
    pub fn fadd_m(&mut self, v: f64) {
        *self.top() = self.st0() + F80::from_f64(v);
    }
    pub fn fsub_m(&mut self, v: f64) {
        *self.top() = self.st0() - F80::from_f64(v);
    }
    pub fn fsubr_m(&mut self, v: f64) {
        *self.top() = F80::from_f64(v) - self.st0();
    }
    pub fn fdiv_m(&mut self, v: f64) {
        *self.top() = self.st0() / F80::from_f64(v);
    }
    pub fn fmulp(&mut self, i: usize) {
        self.fmul_to(i);
        self.pop();
    }
    pub fn faddp(&mut self, i: usize) {
        let k = self.at(i);
        self.0[k] = self.0[k] + self.st0();
        self.pop();
    }
    /// `fsubp st(i),st`: `st(i) = st(i) - st(0)`, pop.
    pub fn fsubp(&mut self, i: usize) {
        let k = self.at(i);
        self.0[k] = self.0[k] - self.st0();
        self.pop();
    }
    /// `fsubrp st(i),st`: `st(i) = st(0) - st(i)`, pop.
    pub fn fsubrp(&mut self, i: usize) {
        let k = self.at(i);
        self.0[k] = self.st0() - self.0[k];
        self.pop();
    }
}
