//! Draft 05 repro: a throwing/`Result`-returning `#[export]` method that returns a class handle.
//! The original claim was that `Result<ClassHandle, E>` fails to compile (`Handle: WireEncode`
//! not satisfied). This crate is the smallest faithful shape — an `&self` method on one exported
//! class returning `Result<a DIFFERENT exported class, error DTO>` — and it COMPILES at both
//! boltffi 0.27.5 and `=0.27.3`. See ../05-throwing-method-cannot-return-class-handle.md.
use boltffi::*;

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MakeError {
    pub reason: String,
}

pub struct Store {}
pub struct Draft {
    value: u32,
}

#[export]
impl Store {
    pub fn new() -> Store {
        Store {}
    }

    /// The exact draft-05 shape: a fallible `&self` method returning a *different* class handle.
    pub fn try_make(&self, seed: u32) -> Result<Draft, MakeError> {
        if seed == 0 {
            Err(MakeError {
                reason: "zero seed".to_string(),
            })
        } else {
            Ok(Draft { value: seed })
        }
    }
}

#[export]
impl Draft {
    pub fn value(&self) -> u32 {
        self.value
    }
}
