use solver::{CDCL, CO_LBD_BOUND, SearchStrategy, Solver, Stat};
use std::cmp::Ordering;
use std::f64;
use std::fmt;
use std::usize::MAX;
use types::*;
use var::{Satisfiability, Var};

/// for ClauseIndex
pub trait ClauseList {
    fn push(&mut self, cix: ClauseIndex, list: &mut ClauseIndex) -> ClauseIndex;
    fn push_garbage(&mut self, c: &mut Clause, index: usize) -> ClauseIndex;
}

/// for ClausePack
pub trait GC {
    fn garbage_collect(&mut self) -> ();
    fn new_clause(&mut self, v: &[Lit], rank: usize, learnt: bool, locked: bool) -> ClauseId;
    fn reset_lbd(&mut self, vars: &[Var]) -> ();
}

/// for usize
pub trait ClauseIdIndexEncoding {
    fn to_id(&self) -> ClauseId;
    fn to_index(&self) -> ClauseIndex;
    fn to_kind(&self) -> usize;
}

/// for Solver
pub trait ClauseManagement {
    fn bump_cid(&mut self, ci: ClauseId) -> ();
    fn decay_cla_activity(&mut self) -> ();
    fn add_clause(&mut self, v: &mut Vec<Lit>) -> bool;
    fn add_learnt(&mut self, v: &mut Vec<Lit>, lbd: usize) -> ClauseId;
    fn reduce(&mut self) -> ();
    fn simplify(&mut self) -> bool;
    fn lbd_vec(&mut self, v: &[Lit]) -> usize;
    fn lbd_of(&mut self, c: &Clause) -> usize;
}

// const DB_INIT_SIZE: usize = 1000;
const DB_INC_SIZE: usize = 200;

pub const CLAUSE_KINDS: [ClauseKind; 3] = [
    ClauseKind::Removable,
    ClauseKind::Permanent,
    ClauseKind::Binclause,
];

/// partition of clauses
#[derive(Debug)]
pub struct ClausePack {
    pub kind: ClauseKind,
    pub init_size: usize,
    pub clauses: Vec<Clause>,
    pub touched: Vec<bool>,
    pub permutation: Vec<ClauseIndex>,
    pub watcher: Vec<ClauseIndex>,
    pub tag: usize,
    pub mask: usize,
    pub index_bits: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClauseKind {
    Removable = 0,
    Permanent,
    Binclause,
}

const CLAUSE_INDEX_BITS: usize = 60;
const CLAUSE_INDEX_MASK: usize = 0x0FFF_FFFF_FFFF_FFFF;
pub const DEAD_CLAUSE: usize = MAX;

impl ClauseKind {
    pub fn tag(self) -> usize {
        match self {
            ClauseKind::Removable => 0x0000_0000_0000_0000,
            ClauseKind::Permanent => 0x1000_0000_0000_0000,
            ClauseKind::Binclause => 0x2000_0000_0000_0000,
        }
    }
    pub fn mask(self) -> usize {
        CLAUSE_INDEX_MASK
    }
    pub fn id_from(self, cix: ClauseIndex) -> ClauseId {
        cix | self.tag()
    }
    pub fn index_from(self, cid: ClauseId) -> ClauseIndex {
        cid & self.mask()
    }
}

impl ClausePack {
    pub fn build(kind: ClauseKind, nv: usize, nc: usize) -> ClausePack {
        let tag = kind.tag();
        let mask = kind.mask();
        let mut clauses = Vec::with_capacity(1 + nc);
        clauses.push(Clause::null());
        let mut permutation = Vec::new();
        permutation.push(0); // for NULL_CLAUSE
        let mut watcher = Vec::with_capacity(2 * (nv + 1));
        let mut touched = Vec::with_capacity(2 * (nv + 1));
        for _i in 0..2 * (nv + 1) {
            watcher.push(NULL_CLAUSE);
            touched.push(false);
        }
        ClausePack {
            kind,
            init_size: nc,
            clauses,
            touched,
            permutation,
            watcher,
            mask,
            tag,
            index_bits: CLAUSE_INDEX_BITS,
        }
    }
    pub fn attach(&mut self, mut c: Clause) -> ClauseId {
        let w0 = c.lit[0].negate() as usize;
        let w1 = c.lit[1].negate() as usize;
        let cix = self.clauses.len();
        c.index = cix;
        c.flags &= !3;
        c.flags |= self.kind as u32;
        self.permutation.push(cix);
        c.next_watcher[0] = self.watcher[w0];
        self.watcher[w0] = cix;
        c.next_watcher[1] = self.watcher[w1];
        self.watcher[w1] = cix;
        self.clauses.push(c);
        self.id_from(cix)
    }
    pub fn id_from(&self, cix: ClauseIndex) -> ClauseId {
        cix | self.tag
    }
    pub fn index_from(&self, cid: ClauseId) -> ClauseIndex {
        cid & self.mask
    }
    pub fn len(&self) -> usize {
        self.clauses.len()
    }
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }
    pub fn count(&self, target: Lit, limit: usize) -> usize {
        let mut cnt = 0;
        for _ in self.iter_watcher(target) {
            cnt += 1;
            if 0 < limit && limit <= cnt {
                return limit;
            }
        }
        cnt
    }
}

impl Clause {
    pub fn get_kind(&self) -> ClauseKind {
        match self.flags & 3 {
            0 => ClauseKind::Removable,
            1 => ClauseKind::Permanent,
            2 => ClauseKind::Binclause,
            _ => panic!("impossible clause kind"),
        }
    }
    pub fn set_flag(&mut self, flag: ClauseFlag, val: bool) -> () {
        self.flags &= !(1 << (flag as u32));
        self.flags |= (val as u32) << (flag as u32);
    }
    pub fn get_flag(&self, flag: ClauseFlag) -> bool {
        self.flags & (1 << flag as u32) != 0
    }
}

pub const RANK_NULL: usize = 0; // for NULL_CLAUSE
pub const RANK_CONST: usize = 1; // for given clauses
pub const RANK_NEED: usize = 2; // for newly generated bi-clauses

/// Clause Index, not ID because it's used only within a Vec<Clause>
pub type ClauseIndex = usize;

/// Clause
#[derive(Debug)]
pub struct Clause {
    /// index (not id), used in a CP.
    pub index: ClauseIndex,
    /// LBD or NDD and so on, used by `reduce_db`
    pub lit: [Lit; 2],
    /// the literals without lit0 and lit1
    pub next_watcher: [ClauseIndex; 2],
    /// The first two literals
    pub lits: Vec<Lit>,
    /// for `ClauseFlag`
    pub rank: usize,
    /// ClauseIndexes of the next in the watch liss
    pub flags: u32,
    /// clause activity used by `analyze` and `reduce_db`
    pub activity: f64,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ClauseFlag {
    Kind0 = 0,
    Kind1,
    Dead,
    Locked,
    Learnt,
    JustUsed,
    SveMark,
    Touched,
}

impl ClauseFlag {
    fn as_bit(self, val: bool) -> u32 {
        (val as u32) << (self as u32)
    }
}

impl ClauseIdIndexEncoding for usize {
    fn to_id(&self) -> ClauseId {
        *self
    }
    #[inline]
    fn to_index(&self) -> ClauseIndex {
        (*self & CLAUSE_INDEX_MASK) as usize
    }
    #[inline]
    fn to_kind(&self) -> usize {
        *self >> CLAUSE_INDEX_BITS
    }
}

impl PartialEq for Clause {
    fn eq(&self, other: &Clause) -> bool {
        self.index == other.index
    }
}

impl Eq for Clause {}

impl PartialOrd for Clause {
    fn partial_cmp(&self, other: &Clause) -> Option<Ordering> {
        if self.rank < other.rank {
            Some(Ordering::Less)
        } else if self.rank > other.rank {
            Some(Ordering::Greater)
        } else if self.activity > other.activity {
            Some(Ordering::Less)
        } else if self.activity < other.activity {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Equal)
        }
    }
}

impl Ord for Clause {
    fn cmp(&self, other: &Clause) -> Ordering {
        if self.rank < other.rank {
            Ordering::Less
        } else if self.rank > other.rank {
            Ordering::Greater
        } else if self.activity > other.activity {
            Ordering::Less
        } else if self.activity < other.activity {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl Clause {
    pub fn new(kind: ClauseKind, learnt: bool, rank: usize, v: &[Lit]) -> Clause {
        let mut v = v.to_owned();
        let lit0 = v.remove(0);
        let lit1 = v.remove(0);
        Clause {
            activity: 0.0,
            rank,
            next_watcher: [NULL_CLAUSE; 2],
            lit: [lit0, lit1],
            lits: v,
            index: 0,
            flags: (kind as u32) | ClauseFlag::Learnt.as_bit(learnt),
        }
    }
    pub fn null() -> Clause {
        Clause {
            //            kind: ClauseKind::Permanent,
            activity: 0.0,
            rank: RANK_NULL,
            next_watcher: [NULL_CLAUSE; 2],
            lit: [NULL_LIT; 2],
            lits: vec![],
            index: 0,
            flags: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.lits.len() + 2
    }
    pub fn is_empty(&self) -> bool {
        false
    }
}

impl fmt::Display for Clause {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if f.alternate() {
            write!(
                f,
                "{{C{}:{} lit:{:?}{:?}, watches{:?}{}{}}}",
                self.flags & 3,
                self.index,
                vec2int(&self.lit),
                vec2int(&self.lits),
                self.next_watcher,
                if self.get_flag(ClauseFlag::Dead) {
                    ", dead"
                } else {
                    ""
                },
                if self.get_flag(ClauseFlag::Locked) {
                    ", locked"
                } else {
                    ""
                },
            )
        } else {
            match self.index {
                //            x if x < 0 => write!(f, format!("a given clause {}", self.lits.map(|l| l.int()))),
                0 => write!(f, "null_clause"),
                DEAD_CLAUSE => write!(
                    f,
                    "dead[{},{}]{:?}",
                    self.lit[0].int(),
                    self.lit[1].int(),
                    &self.lits.iter().map(|l| l.int()).collect::<Vec<i32>>()
                ),
                _ if self.lits.is_empty() => write!(
                    f,
                    "B{}[{},{}]",
                    self.index,
                    self.lit[0].int(),
                    self.lit[1].int(),
                ),
                _ => write!(
                    f,
                    "{}{}[{},{}]{:?}",
                    match self.flags & 3 {
                        0 => 'L',
                        1 => 'P',
                        2 => 'B',
                        _ => '?',
                    },
                    self.index,
                    self.lit[0].int(),
                    self.lit[1].int(),
                    &self.lits.iter().map(|l| l.int()).collect::<Vec<i32>>(),
                ),
            }
        }
    }
}

pub fn cid2fmt(cid: ClauseId) -> String {
    match cid >> CLAUSE_INDEX_BITS {
        0 => format!("[learnt:{}]", cid.to_index()),
        _ => format!("[prmnnt:{}]", cid.to_index()),
    }
}

impl Clause {
    pub fn subsumes(&self, other: &Clause) -> Option<Lit> {
        let mut ret: Lit = NULL_LIT;
        'next: for i in 0..self.len() {
            let l = lindex!(self, i);
            for j in 0..other.len() {
                let lo = lindex!(other, j);
                if l == lo {
                    continue 'next;
                } else if ret == NULL_LIT && l == lo.negate() {
                    ret = l;
                    continue 'next;
                }
            }
            return None;
        }
        Some(ret)
    }
    /// remove Lit `p` from Clause *self*.
    /// returns true if the clause became a unit clause.
    pub fn strengthen(&mut self, p: Lit) -> bool {
        if self.get_flag(ClauseFlag::Dead) {
            return false;
        }
        let len = self.len();
        if len == 2 {
            if self.lit[0] == p {
                self.lit.swap(0, 1);
            }
            return true;
        }
        if self.lit[0] == p {
            self.lit[0] = self.lits.pop().unwrap();
        } else if self.lit[1] == p {
            self.lit[1] = self.lits.pop().unwrap();
        } else {
            self.lits.retain(|&x| x != p);
        }
        false
    }
}

pub struct ClauseIter<'a> {
    clause: &'a Clause,
    end: usize,
    index: usize,
}

impl<'a> IntoIterator for &'a Clause {
    type Item = Lit;
    type IntoIter = ClauseIter<'a>;
    fn into_iter(self) -> ClauseIter<'a> {
        ClauseIter {
            clause: &self,
            end: self.len(),
            index: 0,
        }
    }
}

impl<'a> Iterator for ClauseIter<'a> {
    type Item = Lit;
    fn next(&mut self) -> Option<Lit> {
        self.index += 1;
        match self.index {
            1 => Some(self.clause.lit[0]),
            2 => Some(self.clause.lit[1]),
            n if n <= self.end => Some(self.clause.lits[n - 3]),
            _ => None,
        }
    }
}

impl ClauseManagement for Solver {
    #[inline]
    fn bump_cid(&mut self, cid: ClauseId) -> () {
        debug_assert_ne!(cid, 0);
        let a;
        {
            // let c = &mut self.cp[cid.to_kind()].clauses[cid.to_index()];
            let c = mref!(self.cp, cid);
            a = c.activity + self.cla_inc;
            c.activity = a;
        }
        if 1.0e20 < a {
            for c in &mut self.cp[ClauseKind::Removable as usize].clauses {
                if c.get_flag(ClauseFlag::Learnt) {
                    c.activity *= 1.0e-20;
                }
            }
            self.cla_inc *= 1.0e-20;
        }
    }
    fn decay_cla_activity(&mut self) -> () {
        self.cla_inc /= self.config.clause_decay_rate;
    }
    // renamed from clause_new
    fn add_clause(&mut self, v: &mut Vec<Lit>) -> bool {
        v.sort_unstable();
        let mut j = 0;
        let mut l_ = NULL_LIT; // last literal; [x, x.negate()] means totology.
        for i in 0..v.len() {
            let li = v[i];
            let sat = self.vars.assigned(li);
            if sat == LTRUE || li.negate() == l_ {
                return true;
            } else if sat != LFALSE && li != l_ {
                v[j] = li;
                j += 1;
                l_ = li;
            }
        }
        v.truncate(j);
        let kind = if v.len() == 2 {
            ClauseKind::Binclause
        } else {
            ClauseKind::Permanent
        };
        match v.len() {
            0 => false, // Empty clause is UNSAT.
            1 => self.enqueue(v[0], NULL_CLAUSE),
            _ => {
                self.cp[kind as usize].new_clause(&v, 0, false, false);
                true
            }
        }
    }
    /// renamed from newLearntClause
    fn add_learnt(&mut self, v: &mut Vec<Lit>, lbd: usize) -> ClauseId {
        debug_assert_ne!(v.len(), 0);
        if v.len() == 1 {
            self.uncheck_enqueue(v[0], NULL_CLAUSE);
            return 0;
        }
        // let lbd = v.lbd(&self.vars, &mut self.lbd_seen);
        let mut i_max = 0;
        let mut lv_max = 0;
        // seek a literal with max level
        for (i, l) in v.iter().enumerate() {
            let vi = l.vi();
            let lv = self.vars[vi].level;
            if self.vars[vi].assign != BOTTOM && lv_max < lv {
                i_max = i;
                lv_max = lv;
            }
        }
        v.swap(1, i_max);
        let l0 = v[0];
        let kind = if v.len() == 2 {
            ClauseKind::Binclause
        } else if self.strategy == Some(SearchStrategy::ChanSeok) && lbd <= CO_LBD_BOUND {
            ClauseKind::Permanent
        } else {
            ClauseKind::Removable
        };
        let cid = self.cp[kind as usize].new_clause(&v, lbd, true, true);
        self.bump_cid(cid);
        self.uncheck_enqueue(l0, cid);
        cid
    }

    fn reduce(&mut self) -> () {
        {
            let ClausePack {
                ref mut clauses,
                ref mut touched,
                ..
            } = &mut self.cp[ClauseKind::Removable as usize];
            let permutation = &mut (1..clauses.len())
                .filter(|i| !clauses[*i].get_flag(ClauseFlag::Dead)
                        && !clauses[*i].get_flag(ClauseFlag::Locked)
                ) // garbage and recycled
                .collect::<Vec<ClauseIndex>>();
            debug_assert!(!permutation.is_empty());
            permutation[1..].sort_by(|&a, &b| clauses[a].cmp(&clauses[b]));
            let nc = permutation.len();
            let keep = nc / 2;
            if clauses[permutation[keep]].rank <= 4 {
                self.next_reduction += 1000;
            };
            for i in keep..nc {
                let mut c = &mut clauses[permutation[i]];
                if c.get_flag(ClauseFlag::JustUsed) {
                    c.set_flag(ClauseFlag::JustUsed, false)
                } else {
                    c.set_flag(ClauseFlag::Dead, true);
                    touched[c.lit[0].negate() as usize] = true;
                    touched[c.lit[1].negate() as usize] = true;
                }
            }
        }
        // self.garbage_collect(ClauseKind::Removable);
        self.cp[ClauseKind::Removable as usize].garbage_collect();
        self.cp[ClauseKind::Removable as usize].reset_lbd(&self.vars);
        self.next_reduction += DB_INC_SIZE;
        self.stats[Stat::Reduction as usize] += 1;
    }

    fn simplify(&mut self) -> bool {
        debug_assert_eq!(self.decision_level(), 0);
        if self.eliminator.use_elim
            && self.stats[Stat::Simplification as usize] % 8 == 0
            && self.eliminator.last_invocatiton < self.stats[Stat::Reduction as usize] as usize
        {
            // self.eliminate();
            self.eliminator.last_invocatiton = self.stats[Stat::Reduction as usize] as usize;
        }
        // reset reason since decision level is zero.
        for v in &mut self.vars {
            if v.reason != NULL_CLAUSE {
                self.cp[v.reason.to_kind()].clauses[v.reason.to_index()]
                    .set_flag(ClauseFlag::Locked, false);
                v.reason = NULL_CLAUSE;
            }
        }
        for ck in &CLAUSE_KINDS {
            for c in &mut self.cp[*ck as usize].clauses[1..] {
                c.rank = c.len();
                if self.vars.satisfies(c) {
                    c.set_flag(ClauseFlag::Dead, true);
                    self.cp[*ck as usize].touched[c.lit[0].negate() as usize] = true;
                    self.cp[*ck as usize].touched[c.lit[1].negate() as usize] = true;
                }
            }
            // for (lit, start) in self.cp[*ck as usize].watcher.iter().enumerate().skip(2) {
            //     let neg = (lit as Lit).negate();
            //     if self.vars.assigned(neg) == LTRUE {
            //         self.cp[*ck as usize].touched[lit] = true;
            //         let mut ci = *start;
            //         while ci != NULL_CLAUSE {
            //             let c = &mut self.cp[*ck as usize].clauses[ci];
            //             debug_assert!(!c.get_flag(ClauseFlag::Locked));
            //             c.set_flag(ClauseFlag::Dead, true);
            //             self.cp[*ck as usize].touched[c.lit[(c.lit[0] == neg) as usize].negate() as usize] = true;
            //             ci = c.next_watcher[(c.lit[0] != neg) as usize];
            //         }
            //     }
            // }
            self.cp[*ck as usize].garbage_collect();
            // self.garbage_collect(*ck);
        }
        self.stats[Stat::Simplification as usize] += 1;
        //        if self.eliminator.use_elim
        //            && self.stats[Stat::Simplification as usize] % 8 == 0
        //            && self.eliminator.last_invocatiton < self.stats[Stat::Reduction as usize] as usize
        //        {
        //            self.eliminate();
        //            self.eliminator.last_invocatiton = self.stats[Stat::Reduction as usize] as usize;
        //            for ck in &KINDS {
        //                // self.garbage_collect(*ck);
        //                self.cp[*ck as usize].garbage_collect();
        //            }
        //        }
        true
    }
    fn lbd_vec(&mut self, v: &[Lit]) -> usize {
        let key;
        let key_old = self.lbd_seen[0];
        if 10_000_000 < key_old {
            key = 1;
        } else {
            key = key_old + 1;
        }
        let mut cnt = 0;
        for l in v {
            let lv = self.vars[l.vi()].level;
            if self.lbd_seen[lv] != key && lv != 0 {
                self.lbd_seen[lv] = key;
                cnt += 1;
            }
        }
        self.lbd_seen[0] = key;
        cnt
    }
    fn lbd_of(&mut self, c: &Clause) -> usize {
        let key;
        let key_old = self.lbd_seen[0];
        if 10_000_000 < key_old {
            key = 1;
        } else {
            key = key_old + 1;
        }
        let mut cnt = 0;
        for l in &c.lit {
            let lv = self.vars[l.vi()].level;
            if self.lbd_seen[lv] != key && lv != 0 {
                self.lbd_seen[lv] = key;
                cnt += 1;
            }
        }
        for l in &c.lits {
            let lv = self.vars[l.vi()].level;
            if self.lbd_seen[lv] != key && lv != 0 {
                self.lbd_seen[lv] = key;
                cnt += 1;
            }
        }
        self.lbd_seen[0] = key;
        cnt
    }
}

impl Solver {
    // # Prerequisite
    /// - `ClausePack.clauses` has dead clauses, and their index fields hold valid vaule.
    /// - `Caluse.index` of all the dead clauses is DEAD_CLAUSE.
    /// - `ClausePack.permutation` is valid and can be destoried here.
    ///
    /// # Result
    /// - `ClausePack.clauses` has only active clauses, and their sorted with new index.
    /// - `ClausePack.permutation` is sorted.
    /// - `Var.reason` is updated with new clause ids.
    /// - By calling `rebuild_watchers`, All `ClausePack.watcher` hold valid links.
    pub fn garbage_collect_compaction(&mut self, kind: ClauseKind) -> () {
        let dl = self.decision_level();
        {
            let ClausePack {
                ref mut clauses,
                ref mut permutation,
                ..
            } = &mut self.cp[kind as usize];
            // set new indexes to index field of active clauses.
            let mut ni = 0; // new index
            for c in &mut *clauses {
                if !c.get_flag(ClauseFlag::Dead) {
                    c.index = ni;
                    ni += 1;
                }
            }
            // rebuild reason
            if dl == 0 {
                for v in &mut self.vars[1..] {
                    v.reason = NULL_CLAUSE;
                }
            } else {
                for v in &mut self.vars[1..] {
                    let cid = v.reason;
                    if 0 < cid && cid.to_kind() == kind as usize {
                        v.reason = kind.id_from(clauses[cid].index);
                    }
                }
            }
            // GC
            clauses.retain(|ref c| !c.get_flag(ClauseFlag::Dead));
            // rebuild permutation
            permutation.clear();
            for (i, _) in clauses.iter().enumerate() {
                debug_assert_eq!(clauses[i].index, i);
                permutation.push(i);
            }
        }
        self.rebuild_watchers(kind);
    }
    pub fn rebuild_watchers(&mut self, kind: ClauseKind) -> () {
        let ClausePack {
            ref mut clauses,
            ref mut watcher,
            ..
        } = &mut self.cp[kind as usize];
        for mut x in &mut *watcher {
            *x = NULL_CLAUSE;
        }
        for mut c in &mut *clauses {
            if c.get_flag(ClauseFlag::Dead) || c.index == DEAD_CLAUSE {
                continue;
            }
            let w0 = c.lit[0].negate() as usize;
            c.next_watcher[0] = watcher[w0];
            watcher[w0] = c.index;
            let w1 = c.lit[1].negate() as usize;
            c.next_watcher[1] = watcher[w1];
            watcher[w1] = c.index;
        }
    }
}

impl GC for ClausePack {
    fn garbage_collect(&mut self) -> () {
        // let mut ci = self.watcher[GARBAGE_LIT.negate() as usize];
        // while ci != NULL_CLAUSE {
        //     let c = &self.clauses[ci];
        //     debug_assert!(c.dead);
        //     debug_assert!(c.lit[0] == GARBAGE_LIT || c.lit[1] == GARBAGE_LIT);
        //     let index = (c.lit[0] != GARBAGE_LIT) as usize;
        //     ci = c.next_watcher[index];
        // }
        unsafe {
            let garbages = &mut self.watcher[GARBAGE_LIT.negate() as usize] as *mut ClauseId;
            for l in 2..self.watcher.len() {
                if self.touched[l] {
                    self.touched[l] = false;
                } else {
                    continue;
                }
                let vi = (l as Lit).vi();
                let mut pri = &mut self.watcher[l] as *mut ClauseId;
                let mut ci = self.watcher[l];
                while ci != NULL_CLAUSE {
                    let c = &mut self.clauses[ci] as *mut Clause;
                    if !(*c).get_flag(ClauseFlag::Dead) {
                        pri = &mut (*c).next_watcher[((*c).lit[0].vi() != vi) as usize];
                    } else {
                        // debug_assert!((*c).lit[0] == GARBAGE_LIT || (*c).lit[1] == GARBAGE_LIT);
                        debug_assert!((*c).lit[0].negate() == l as Lit || (*c).lit[1].negate() == l as Lit);
                        *pri = (*garbages).push_garbage(&mut *c, ((*c).lit[0].negate() != l as Lit) as usize);
                    }
                    ci = *pri;
                }
            }
            // recycle garbages
            let recycled = &mut self.watcher[RECYCLE_LIT.negate() as usize] as *mut ClauseId;
            let mut pri = &mut self.watcher[GARBAGE_LIT.negate() as usize] as *mut ClauseId;
            let mut ci = self.watcher[GARBAGE_LIT.negate() as usize];
            while ci != NULL_CLAUSE {
                let c = &mut self.clauses[ci];
                debug_assert!(c.get_flag(ClauseFlag::Dead));
                if c.lit[0] == GARBAGE_LIT && c.lit[1] == GARBAGE_LIT {
                    let next = c.next_watcher[0];
                    *pri = c.next_watcher[0];
                    c.lit[0] = RECYCLE_LIT;
                    c.lit[1] = RECYCLE_LIT;
                    c.next_watcher[0] = *recycled;
                    c.next_watcher[1] = *recycled;
                    *recycled = ci;
                    c.set_flag(ClauseFlag::Locked, true);
                    ci = next;
                } else {
                    debug_assert!(c.lit[0] == GARBAGE_LIT || c.lit[1] == GARBAGE_LIT);
                    let index = (c.lit[0] != GARBAGE_LIT) as usize;
                    ci = c.next_watcher[index];
                    pri = &mut c.next_watcher[index];
                }
            }
        }
        debug_assert_eq!(self.watcher[GARBAGE_LIT.negate() as usize], NULL_CLAUSE);
        // // ASSERTION
        // {
        //     for i in 2..self.watcher.len() {
        //         let mut ci = self.watcher[i];
        //         while ci != NULL_CLAUSE {
        //             if self.clauses[ci].dead {
        //                 panic!("aeaaeamr");
        //             }
        //             let index = self.clauses[ci].lit[0].negate() != i as Lit;
        //             ci = self.clauses[ci].next_watcher[index as usize];
        //         }
        //     }
        // }
    }
    fn new_clause(&mut self, v: &[Lit], rank: usize, learnt: bool, locked: bool) -> ClauseId {
        let cix;
        let w0;
        let w1;
        if self.watcher[RECYCLE_LIT.negate() as usize] != NULL_CLAUSE {
            cix = self.watcher[RECYCLE_LIT.negate() as usize];
            debug_assert_eq!(self.clauses[cix].get_flag(ClauseFlag::Dead), true);
            debug_assert_eq!(self.clauses[cix].lit[0], RECYCLE_LIT);
            debug_assert_eq!(self.clauses[cix].lit[1], RECYCLE_LIT);
            debug_assert_eq!(self.clauses[cix].index, cix);
            self.watcher[RECYCLE_LIT.negate() as usize] = self.clauses[cix].next_watcher[0];
            let c = &mut self.clauses[cix];
            c.lit[0] = v[0];
            c.lit[1] = v[1];
            c.lits.clear();
            for l in &v[2..] {
                c.lits.push(*l);
            }
            c.rank = rank;
            c.flags = 0; // reset Dead, JustUsed, SveMark and Touched
            c.set_flag(ClauseFlag::Locked, locked);
            c.set_flag(ClauseFlag::Learnt, learnt);
            c.activity = 0.0;
            w0 = c.lit[0].negate() as usize;
            w1 = c.lit[1].negate() as usize;
            c.next_watcher[0] = self.watcher[w0];
            c.next_watcher[1] = self.watcher[w1];
        } else {
            let mut c = Clause::new(self.kind, learnt, rank, &v);
            c.set_flag(ClauseFlag::Locked, locked);
            cix = self.clauses.len();
            c.index = cix;
            w0 = c.lit[0].negate() as usize;
            w1 = c.lit[1].negate() as usize;
            c.next_watcher[0] = self.watcher[w0];
            c.next_watcher[1] = self.watcher[w1];
            self.clauses.push(c);
        };
        self.watcher[w0] = cix;
        self.watcher[w1] = cix;
        self.id_from(cix)
    }
    fn reset_lbd(&mut self, vars: &[Var]) -> () {
        let mut temp = Vec::with_capacity(vars.len());
        for _ in 0..vars.len() {
            temp.push(0);
        }
        for c in &mut self.clauses[1..] {
            if c.get_flag(ClauseFlag::Dead) {
                continue;
            }
            let key = c.index;
            let mut cnt = 0;
            for l in &c.lit {
                let lv = vars[l.vi()].level;
                if temp[lv] != key && lv != 0 {
                    temp[lv] = key;
                    cnt += 1;
                }
            }
            for l in &c.lits {
                let lv = vars[l.vi()].level;
                if temp[lv] != key && lv != 0 {
                    temp[lv] = key;
                    cnt += 1;
                }
            }
            c.rank = cnt;
        }
    }
}

impl ClauseList for ClauseIndex {
    #[inline(always)]
    fn push(&mut self, cix: ClauseIndex, item: &mut ClauseIndex) -> ClauseIndex {
        *item = *self;
        *self = cix;
        *item
    }
    fn push_garbage(&mut self, c: &mut Clause, index: usize) -> ClauseIndex {
        debug_assert!(index == 0 || index == 1);
        let other = (index ^ 1) as usize;
        debug_assert!(other == 0 || other == 1);
        c.lit[index] = GARBAGE_LIT;
        let next = c.next_watcher[index];
        if c.lit[other] == GARBAGE_LIT {
            c.next_watcher[index] = c.next_watcher[other];
        } else {
            self.push(c.index, &mut c.next_watcher[index]);
        }
        next
    }
}

pub struct ClauseListIter<'a> {
    vec: &'a Vec<Clause>,
    target: Lit,
    next_index: ClauseIndex,
}

impl ClausePack {
    pub fn iter_watcher(&self, p: Lit) -> ClauseListIter {
        ClauseListIter {
            vec: &self.clauses,
            target: p,
            next_index: self.watcher[p.negate() as usize],
        }
    }
}

impl<'a> Iterator for ClauseListIter<'a> {
    type Item = &'a Clause;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index == NULL_CLAUSE {
            None
        } else {
            let c = &self.vec[self.next_index as usize];
            self.next_index = c.next_watcher[(c.lit[0] != self.target) as usize];
            Some(&self.vec[self.next_index as usize])
        }
    }
}
