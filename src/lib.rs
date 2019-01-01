//! This is a SAT solver in Rust.

#[macro_export]
macro_rules! clause {
    ($cv: expr, $val: expr) => {{
        match (&$cv, $val) {
            (v, cid) => &v[cid.to_kind()].head[cid.to_index()],
        }
    }};
}

#[macro_export]
macro_rules! clause_mut {
    ($cv: expr, $val: expr) => {{
        match (&mut $cv, $val) {
            (v, cid) => &mut v[cid.to_kind()].head[cid.to_index()],
        }
    }};
}

#[allow(unused_macros)]
macro_rules! uenqueue {
    ($vs: expr, $tr: expr, $tl: expr, $lit: expr, $cid: expr) => {{
        match (&$vs, &mut $tr, &$tl, $lit, $cid) {
            (vs, tr, tl, lit, cid) => {
                let mut v = &mut vs[lit.vi()];
                v.assign = lit.lbool();
                v.level = tl.len();
                v.reason = cid;
                tr.push(lit);
            }
        }
    }};
}

// /// Subsumption-based clause/var eliminaiton
/// Assignment management
pub mod assign;
/// Clause
pub mod clause;
/// In-process elimination
pub mod eliminator;
/// used in progress report
pub mod profiler;
/// Implementation on solver restart.
pub mod restart;
/// struct Solver
pub mod solver;
/// Plumping layer.
pub mod types;
/// validates
pub mod validator;
/// Var
pub mod var;
