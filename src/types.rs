//! Basic types
use std::fmt;

/// Variable as Index is `usize`
pub type VarId = usize;

/// Clause Identifier. Note: it changes after database reduction.
pub type ClauseId = usize;

/// is a dummy clause index
pub const NULL_CLAUSE: ClauseId = 0;

/// Literal encoded on unsigned integer
/// # Examples
///
/// ```
/// use splr::types::*;
/// assert_eq!(2, int2lit( 1) as i32);
/// assert_eq!(3, int2lit(-1) as i32);
/// assert_eq!(4, int2lit( 2) as i32);
/// assert_eq!(5, int2lit(-2) as i32);
/// assert_eq!( 1, int2lit( 1).int());
/// assert_eq!(-1, int2lit(-1).int());
/// assert_eq!( 2, int2lit( 2).int());
/// assert_eq!(-2, int2lit(-2).int());
/// ```
pub type Lit = u32;

/// a dummy literal.
pub const NULL_LIT: Lit = 0;

pub fn int2lit(x: i32) -> Lit {
    (if x < 0 { -2 * x + 1 } else { 2 * x }) as u32
}

/// Converters between 'int', [Lit](type.Lit.html) and [Var](type.Var.html).
/// # Examples
///
/// ```
/// use splr::types::*;
/// assert_eq!(int2lit(1), 1.lit(LTRUE));
/// assert_eq!(int2lit(2), 2.lit(LTRUE));
/// assert_eq!(1, 1.lit(LTRUE).vi());
/// assert_eq!(1, 1.lit(LFALSE).vi());
/// assert_eq!(2, 2.lit(LTRUE).vi());
/// assert_eq!(2, 2.lit(LFALSE).vi());
/// assert_eq!(int2lit( 1), int2lit(-1).negate());
/// assert_eq!(int2lit(-1), int2lit( 1).negate());
/// assert_eq!(int2lit( 2), int2lit(-2).negate());
/// assert_eq!(int2lit(-2), int2lit( 2).negate());
/// ```
pub trait LiteralEncoding {
    fn vi(&self) -> VarId;
    fn int(&self) -> i32;
    fn lbool(&self) -> Lbool;
    fn positive(&self) -> bool;
    fn negate(&self) -> Lit;
}

impl LiteralEncoding for Lit {
    #[inline]
    fn vi(&self) -> VarId {
        (self / 2) as VarId
    }
    fn int(&self) -> i32 {
        if self % 2 == 0 {
            (*self / 2) as i32
        } else {
            (*self as i32) / -2
        }
    }
    #[inline]
    fn lbool(&self) -> Lbool {
        if self.positive() {
            LTRUE
        } else {
            LFALSE
        }
    }
    #[inline]
    fn positive(&self) -> bool {
        self % 2 == 0
    }
    #[inline]
    fn negate(&self) -> Lit {
        self ^ 1
    }
}

/// converter from [VarId](type.VarId.html) to [Lit](type.Lit.html).
pub trait VarIdEncoding {
    fn lit(&self, p: Lbool) -> Lit;
}

impl VarIdEncoding for VarId {
    #[inline]
    fn lit(&self, p: Lbool) -> Lit {
        (if p == LFALSE { 2 * self + 1 } else { 2 * self }) as Lit
    }
}

/// Lifted Bool type
pub type Lbool = u8;
/// the lifted **false**.
pub const LFALSE: u8 = 0;
/// the lifted **true**.
pub const LTRUE: u8 = 1;
/// unbound bool.
pub const BOTTOM: u8 = 2;

#[inline]
/// Note: this function doesn't work on BOTTOM.
pub fn negate_bool(b: Lbool) -> Lbool {
    b ^ 1
}

/// trait on Ema
pub trait EmaKind {
    /// returns a new EMA from a flag (slow or fast) and a window size
    fn get(&self) -> f64;
    /// returns an EMA value
    fn update(&mut self, x: f64) -> ();
}

/// Exponential Moving Average pair
#[derive(Debug)]
pub struct Ema2 {
    pub fast: f64,
    pub slow: f64,
    pub calf: f64,
    pub cals: f64,
    fe: f64,
    se: f64,
}

impl Ema2 {
    pub fn new(f: f64, s: f64) -> Ema2 {
        Ema2 {
            fast: 0.0,
            slow: 0.0,
            calf: 1.0,
            cals: 1.0,
            fe: 1.0 / f,
            se: 1.0 / s,
        }
    }
}

impl EmaKind for Ema2 {
    fn get(&self) -> f64 {
        self.fast / self.slow * (self.cals / self.calf)
    }
    fn update(&mut self, x: f64) -> () {
        self.fast = &self.fe * x + (1.0 - &self.fe) * &self.fast;
        self.slow = &self.se * x + (1.0 - &self.se) * &self.slow;
        self.calf = &self.fe + (1.0 - &self.fe) * &self.calf;
        self.cals = &self.se + (1.0 - &self.se) * &self.cals;
    }
}

#[derive(Debug)]
pub struct Ema(pub f64, f64, f64);

/// Exponential Moving Average w/ a calibrator
impl Ema {
    pub fn new(s: i32) -> Ema {
        Ema(0.0, 1.0 / s as f64, 0.0)
    }
}

impl EmaKind for Ema {
    fn get(&self) -> f64 {
        self.0 / self.2
    }
    fn update(&mut self, x: f64) -> () {
        let e = &self.1 * x + (1.0 - &self.1) * &self.0;
        self.0 = e;
        let c = &self.1 + (1.0 - &self.1) * &self.2;
        self.2 = c;
    }
}

/// Exponential Moving Average w/o a calibrator
#[derive(Debug)]
pub struct Ema_(pub f64, f64);

impl Ema_ {
    pub fn new(s: i32) -> Ema_ {
        Ema_(0.0, 1.0 / s as f64)
    }
}

impl EmaKind for Ema_ {
    fn get(&self) -> f64 {
        self.0 / self.1
    }
    fn update(&mut self, x: f64) -> () {
        let e = &self.1 * x + (1.0 - &self.1) * &self.0;
        self.0 = e;
    }
}

/// data about a problem.
#[derive(Debug)]
pub struct CNFDescription {
    pub num_of_variables: usize,
    pub num_of_clauses: usize,
    pub pathname: String,
}

impl fmt::Display for CNFDescription {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let CNFDescription {
            num_of_variables: nv,
            num_of_clauses: nc,
            pathname: path,
        } = &self;
        write!(f, "CNF({}, {}, {})", nv, nc, path)
    }
}

#[derive(Debug)]
/// `Solver`'s parameters; random decision rate was dropped.
pub struct SolverConfiguration {
    /// decay rate for variable activity
    pub variable_decay_rate: f64,
    /// decay rate for clause activity
    pub clause_decay_rate: f64,
    /// dump stats data during solving
    pub dump_solver_stat_mode: i32,
    /// the coefficients for restarts
    pub ema_coeffs: (i32, i32),
    /// restart expansion factor
    pub restart_expansion: f64,
    /// static steps between restarts
    pub restart_step: f64,
}

impl Default for SolverConfiguration {
    fn default() -> SolverConfiguration {
        SolverConfiguration {
            variable_decay_rate: 0.95,
            clause_decay_rate: 0.999,
            dump_solver_stat_mode: 0,
            ema_coeffs: (2 ^ 5, 2 ^ 14),
            restart_expansion: 1.15,
            restart_step: 100.0,
        }
    }
}

/// formats of state dump
pub enum DumpMode {
    NoDump = 0,
    DumpCSVHeader,
    DumpCSV,
    DumpJSON,
}
