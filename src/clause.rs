#![allow(unused_variables)]
use crate::eliminator::*;
use crate::solver::{Solver, SolverConfiguration, Stat};
use crate::types::*;
use crate::var::{Var, VarManagement};
use std::cmp::Ordering;
use std::f64;
use std::fmt;

/// For ClausePartition
pub trait GC {
    fn garbage_collect(&mut self, vars: &mut [Var], elimanator: &mut Eliminator) -> ();
    fn new_clause(&mut self, v: &[Lit], rank: usize) -> ClauseId;
    fn reset_lbd(&mut self, vars: &[Var], temp: &mut [usize]) -> ();
    fn move_to(&mut self, list: &mut ClauseId, ci: ClauseIndex, index: usize) -> ClauseIndex;
    fn bump_activity(&mut self, cix: ClauseIndex, val: f64, cla_inc: &mut f64) -> ();
}

/// For usize
pub trait ClauseIdIndexEncoding {
    fn to_id(&self) -> ClauseId;
    fn to_index(&self) -> ClauseIndex;
    fn to_kind(&self) -> usize;
    fn is(&self, kind: ClauseKind, ix: ClauseIndex) -> bool;
}

/// For Solver
pub trait ClauseManagementSolverTemp {
    fn simplify(&mut self) -> bool;
}

/// For ClausePartition
pub trait ConsistencyCheck {
    fn check(&mut self, lit: Lit) -> bool;
}

const DB_INC_SIZE: usize = 200;

/// Clause Index, not ID because it's used only within a Vec<Clause>
pub type ClauseIndex = usize;

pub struct ClauseHead {
    /// Watching literals
    pub lit: [Lit; 2],
    /// pointers to next clauses
    pub next_watcher: [usize; 2],
    /// collection of bits
    pub flags: u16,
    /// the literals
    pub lits: Vec<Lit>,
    /// LBD, NDD, or something, used by `reduce_db`
    pub rank: usize,
    /// clause activity used by `analyze` and `reduce_db`
    pub activity: f64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ClauseFlag {
    Kind0 = 0,
    Kind1,
    Dead,
    JustUsed,
    Enqueued,
}

/// partition of clauses
pub struct ClausePartition {
    pub kind: ClauseKind,
    pub init_size: usize,
    pub head: Vec<ClauseHead>,
    pub perm: Vec<ClauseIndex>,
    pub touched: Vec<bool>,
    pub watcher: Vec<ClauseIndex>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ClauseKind {
    Liftedlit,
    Removable,
    Permanent,
    Binclause,
    Uniclause,
}

const CLAUSE_INDEX_BITS: usize = 60;
const CLAUSE_INDEX_MASK: usize = 0x0FFF_FFFF_FFFF_FFFF;

impl ClauseKind {
    #[inline(always)]
    pub fn tag(self) -> usize {
        match self {
            ClauseKind::Liftedlit => 0x0000_0000_0000_0000,
            ClauseKind::Removable => 0x1000_0000_0000_0000,
            ClauseKind::Permanent => 0x2000_0000_0000_0000,
            ClauseKind::Binclause => 0x3000_0000_0000_0000,
            ClauseKind::Uniclause => 0x4000_0000_0000_0000,
        }
    }
    #[inline(always)]
    pub fn mask(self) -> usize {
        CLAUSE_INDEX_MASK
    }
    #[inline(always)]
    pub fn id_from(self, cix: ClauseIndex) -> ClauseId {
        cix | self.tag()
    }
    #[inline(always)]
    pub fn index_from(self, cid: ClauseId) -> ClauseIndex {
        cid & self.mask()
    }
}

impl ClausePartition {
    pub fn build(kind: ClauseKind, nv: usize, nc: usize) -> ClausePartition {
        let mut head = Vec::with_capacity(1 + nc);
        head.push(ClauseHead {
            lit: [NULL_LIT; 2],
            next_watcher: [NULL_CLAUSE; 2],
            flags: 0,
            lits: vec![],
            rank: 0,
            activity: 0.0,
        });
        let mut perm = Vec::with_capacity(1 + nc);
        perm.push(NULL_CLAUSE);
        let mut watcher = Vec::with_capacity(2 * (nv + 1));
        let mut touched = Vec::with_capacity(2 * (nv + 1));
        for _i in 0..2 * (nv + 1) {
            watcher.push(NULL_CLAUSE);
            touched.push(false);
        }
        ClausePartition {
            kind,
            init_size: nc,
            head,
            perm,
            touched,
            watcher,
        }
    }
    #[inline(always)]
    pub fn id_from(&self, cix: ClauseIndex) -> ClauseId {
        cix | self.kind.tag()
    }
    #[inline(always)]
    pub fn index_from(&self, cid: ClauseId) -> ClauseIndex {
        cid & self.kind.mask()
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

impl ClauseHead {
    #[inline(always)]
    pub fn get_kind(&self) -> ClauseKind {
        match self.flags & 3 {
            0 => ClauseKind::Removable,
            1 => ClauseKind::Permanent,
            2 => ClauseKind::Binclause,
            _ => panic!("impossible clause kind"),
        }
    }
    #[inline(always)]
    pub fn flag_off(&mut self, flag: ClauseFlag) {
        self.flags &= !(1u16 << (flag as u16));
    }
    #[inline(always)]
    pub fn flag_on(&mut self, flag: ClauseFlag) {
        self.flags |= 1u16 << (flag as u16);
    }
    #[inline(always)]
    pub fn set_flag(&mut self, flag: ClauseFlag, val: bool) {
        if val {
            self.flags |= (val as u16) << (flag as u16);
        } else {
            self.flags &= !(1 << (flag as u16));
        }
    }
    #[inline(always)]
    pub fn get_flag(&self, flag: ClauseFlag) -> bool {
        self.flags & (1 << flag as u16) != 0
    }
}

impl ClauseFlag {
    #[allow(dead_code)]
    fn as_bit(self, val: bool) -> u16 {
        (val as u16) << (self as u16)
    }
}

impl ClauseIdIndexEncoding for usize {
    #[inline(always)]
    fn to_id(&self) -> ClauseId {
        *self
    }
    #[inline(always)]
    fn to_index(&self) -> ClauseIndex {
        (*self & CLAUSE_INDEX_MASK) as usize
    }
    #[inline(always)]
    fn to_kind(&self) -> usize {
        *self >> CLAUSE_INDEX_BITS
    }
    #[inline(always)]
    fn is(&self, kind: ClauseKind, ix: ClauseIndex) -> bool {
        (*self).to_kind() == kind as usize && (*self).to_index() == ix
    }
}

impl PartialEq for ClauseHead {
    fn eq(&self, other: &ClauseHead) -> bool {
        self == other
    }
}

impl Eq for ClauseHead {}

impl PartialOrd for ClauseHead {
    #[inline(always)]
    fn partial_cmp(&self, other: &ClauseHead) -> Option<Ordering> {
        if self.rank < other.rank {
            Some(Ordering::Less)
        } else if other.rank < self.rank {
            Some(Ordering::Greater)
        } else if self.activity > other.activity {
            Some(Ordering::Less)
        } else if other.activity > self.activity {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Equal)
        }
    }
}

impl Ord for ClauseHead {
    #[inline(always)]
    fn cmp(&self, other: &ClauseHead) -> Ordering {
        if self.rank < other.rank {
            Ordering::Less
        } else if other.rank > self.rank {
            Ordering::Greater
        } else if self.activity > other.activity {
            Ordering::Less
        } else if other.activity > self.activity {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl fmt::Display for ClauseHead {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "C lit:{:?}, watches:{:?} {{{:?} {}{}{}}}",
            vec2int(&self.lit),
            self.next_watcher,
            vec2int(&self.lits),
            match self.flags & 3 {
                0 => 'L',
                1 => 'R',
                2 => 'P',
                3 => 'B',
                _ => '?',
            },
            if self.get_flag(ClauseFlag::Dead) {
                ", dead"
            } else {
                ""
            },
            if self.get_flag(ClauseFlag::Enqueued) {
                ", enqueued"
            } else {
                ""
            },
        )
    }
}

pub fn cid2fmt(cid: ClauseId) -> String {
    match cid.to_kind() {
        0 if cid == 0 => "NullClause".to_string(),
        0 => format!("Lifted::{}", cid.to_index()),
        1 => format!("Learnt::{}", cid.to_index()),
        2 => format!("Perman::{}", cid.to_index()),
        3 => format!("Binary::{}", cid.to_index()),
        4 => format!("Unicls::{}", cid.to_index()),
        _ => format!("Ilegal::{}", cid.to_index()),
    }
}

pub struct ClauseIter<'a> {
    body: &'a ClauseHead,
    end: usize,
    index: usize,
}

pub fn clause_iter(cb: &ClauseHead) -> ClauseIter {
    ClauseIter {
        body: cb,
        end: cb.lits.len(),
        index: 0,
    }
}

impl<'a> Iterator for ClauseIter<'a> {
    type Item = Lit;
    fn next(&mut self) -> Option<Lit> {
        if self.index < self.end {
            let l = self.body.lits[self.index];
            self.index += 1;
            Some(l)
        } else {
            None
        }
    }
}

impl ClauseManagementSolverTemp for Solver {
    fn simplify(&mut self) -> bool {
        self.cp[ClauseKind::Removable as usize].reset_lbd(&self.vars, &mut self.lbd_temp[..]);
        debug_assert_eq!(self.asgs.level(), 0);
        // reset reason since decision level is zero.
        for v in &mut self.vars[1..] {
            if v.reason != NULL_CLAUSE {
                v.reason = NULL_CLAUSE;
            }
        }
        if self.eliminator.use_elim
        // && self.stat[Stat::Simplification as usize] % 8 == 0
        // && self.eliminator.last_invocatiton < self.stat[Stat::Reduction as usize] as usize
        {
            self.eliminate();
            self.eliminator.last_invocatiton = self.stat[Stat::Reduction as usize] as usize;
            if !self.ok {
                return false;
            }
        }
        {
            let eliminator = &mut self.eliminator;
            let vars = &mut self.vars[..];
            for ck in ClauseKind::Liftedlit as usize..=ClauseKind::Binclause as usize {
                for ch in &mut self.cp[ck].head[1..] {
                    if !ch.get_flag(ClauseFlag::Dead) && vars.satisfies(&ch.lits) {
                        ch.flag_on(ClauseFlag::Dead);
                        debug_assert!(ch.lit[0] != 0 && ch.lit[1] != 0);
                        self.cp[ck].touched[ch.lit[0].negate() as usize] = true;
                        self.cp[ck].touched[ch.lit[1].negate() as usize] = true;
                        if (*eliminator).use_elim {
                            for l in &ch.lits {
                                let v = &mut (*vars)[l.vi()];
                                if !v.eliminated {
                                    (*eliminator).enqueue_var(v);
                                }
                            }
                        }
                    }
                }
                self.cp[ck].garbage_collect(vars, eliminator);
            }
        }
        self.stat[Stat::Simplification as usize] += 1;
        // self.check_eliminator();
        true
    }
}

impl GC for ClausePartition {
    fn garbage_collect(&mut self, vars: &mut [Var], eliminator: &mut Eliminator) {
        unsafe {
            let garbages = &mut self.watcher[GARBAGE_LIT.negate() as usize] as *mut ClauseId;
            for l in 2..self.watcher.len() {
                if self.touched[l] {
                    self.touched[l] = false;
                } else {
                    continue;
                }
                let vi = (l as Lit).vi();
                let mut pri = &mut self.watcher[l] as *mut usize;
                let mut ci = self.watcher[l];
                while ci != NULL_CLAUSE {
                    let ch = &mut self.head[ci] as *mut ClauseHead;
                    if !(*ch).get_flag(ClauseFlag::Dead) {
                        pri = &mut (*ch).next_watcher[((*ch).lit[0].vi() != vi) as usize];
                    } else {
                        debug_assert!(
                            (*ch).lit[0].negate() == l as Lit || (*ch).lit[1].negate() == l as Lit
                        );
                        *pri = self.move_to(
                            &mut *garbages,
                            ci,
                            ((*ch).lit[0].negate() != l as Lit) as usize,
                        );
                    }
                    ci = *pri;
                }
            }
            // recycle garbages
            let recycled = &mut self.watcher[RECYCLE_LIT.negate() as usize] as *mut ClauseId;
            let mut pri = &mut self.watcher[GARBAGE_LIT.negate() as usize] as *mut usize;
            let mut ci = self.watcher[GARBAGE_LIT.negate() as usize];
            while ci != NULL_CLAUSE {
                let cid = self.kind.id_from(ci);
                let ch = &mut self.head[ci];
                debug_assert!(ch.get_flag(ClauseFlag::Dead));
                if ch.lit[0] == GARBAGE_LIT && ch.lit[1] == GARBAGE_LIT {
                    let next = ch.next_watcher[0];
                    *pri = ch.next_watcher[0];
                    ch.lit[0] = RECYCLE_LIT;
                    ch.lit[1] = RECYCLE_LIT;
                    ch.next_watcher[0] = *recycled;
                    ch.next_watcher[1] = *recycled;
                    *recycled = ci;
                    if eliminator.use_elim {
                        for l in &ch.lits {
                            let vi = l.vi();
                            let v = &mut vars[vi];
                            if !v.eliminated {
                                if l.positive() {
                                    v.pos_occurs.retain(|&cj| cid != cj);
                                } else {
                                    v.neg_occurs.retain(|&cj| cid != cj);
                                }
                                eliminator.enqueue_var(v);
                            }
                        }
                    }
                    ci = next;
                } else {
                    debug_assert!(ch.lit[0] == GARBAGE_LIT || ch.lit[1] == GARBAGE_LIT);
                    let index = (ch.lit[0] != GARBAGE_LIT) as usize;
                    ci = ch.next_watcher[index];
                    pri = &mut ch.next_watcher[index];
                }
                ch.lits.clear();
            }
        }
        // debug_assert!(
        //     self.watcher[GARBAGE_LIT.negate() as usize] == NULL_CLAUSE,
        //     format!(
        //         "There's a clause {} {:#} in the GARBAGE list",
        //         cid2fmt(
        //             self.kind
        //                 .id_from(self.watcher[GARBAGE_LIT.negate() as usize])
        //         ),
        //         self.head[self.watcher[GARBAGE_LIT.negate() as usize]],
        //     ),
        // );
    }
    fn new_clause(&mut self, v: &[Lit], rank: usize) -> ClauseId {
        let cix;
        let w0;
        let w1;
        if self.watcher[RECYCLE_LIT.negate() as usize] != NULL_CLAUSE {
            cix = self.watcher[RECYCLE_LIT.negate() as usize];
            debug_assert_eq!(self.head[cix].get_flag(ClauseFlag::Dead), true);
            debug_assert_eq!(self.head[cix].lit[0], RECYCLE_LIT);
            debug_assert_eq!(self.head[cix].lit[1], RECYCLE_LIT);
            self.watcher[RECYCLE_LIT.negate() as usize] = self.head[cix].next_watcher[0];
            let ch = &mut self.head[cix];
            ch.lit[0] = v[0];
            ch.lit[1] = v[1];
            ch.lits.clear();
            for l in &v[..] {
                ch.lits.push(*l);
            }
            ch.rank = rank;
            ch.flags = self.kind as u16; // reset Dead, JustUsed, and Touched
            ch.activity = 1.0;
            w0 = ch.lit[0].negate() as usize;
            w1 = ch.lit[1].negate() as usize;
            ch.next_watcher[0] = self.watcher[w0];
            ch.next_watcher[1] = self.watcher[w1];
        } else {
            let l0 = v[0];
            let l1 = v[1];
            let mut lits = Vec::with_capacity(v.len());
            for l in &v[..] {
                lits.push(*l);
            }
            cix = self.head.len();
            w0 = l0.negate() as usize;
            w1 = l1.negate() as usize;
            self.head.push(ClauseHead {
                lit: [l0, l1],
                next_watcher: [self.watcher[w0], self.watcher[w1]],
                flags: self.kind as u16,
                lits,
                rank,
                activity: 1.0,
            });
            self.perm.push(cix);
        };
        self.watcher[w0] = cix;
        self.watcher[w1] = cix;
        self.id_from(cix)
    }
    fn reset_lbd(&mut self, vars: &[Var], temp: &mut [usize]) {
        let mut key = temp[0];
        for i in 1..self.head.len() {
            let ch = &mut self.head[i];
            if ch.get_flag(ClauseFlag::Dead) {
                continue;
            }
            key += 1;
            let mut cnt = 0;
            for l in &ch.lits {
                let lv = vars[l.vi()].level;
                if temp[lv] != key && lv != 0 {
                    temp[lv] = key;
                    cnt += 1;
                }
            }
            ch.rank = cnt;
        }
        temp[0] = key + 1;
    }
    fn move_to(&mut self, list: &mut ClauseId, ci: ClauseIndex, index: usize) -> ClauseIndex {
        debug_assert!(index == 0 || index == 1);
        let other = (index ^ 1) as usize;
        debug_assert!(other == 0 || other == 1);
        let ch = &mut self.head[ci];
        // let cb = &mut self.body[ci];
        // cb.lits.push(ch.lit[index]); // For valiable eliminator, we copy the literal into body.lits.
        ch.lit[index] = GARBAGE_LIT;
        let next = ch.next_watcher[index];
        if ch.lit[other] == GARBAGE_LIT {
            ch.next_watcher[index] = ch.next_watcher[other];
        } else {
            ch.next_watcher[index] = *list;
            *list = ci;
        }
        next
    }
    fn bump_activity(&mut self, cix: ClauseIndex, val: f64, cla_inc: &mut f64) {
        let c = &mut self.head[cix];
        let a = c.activity + *cla_inc;
        // a = (c.activity + val) / 2.0;
        c.activity = a;
        if 1.0e20 < a {
            for c in self.head.iter_mut().skip(1) {
                if 1.0e-300 < c.activity {
                    c.activity *= 1.0e-20;
                }
            }
            *cla_inc *= 1.0e-20;
        }
    }
}

pub struct ClauseListIter<'a> {
    vec: &'a Vec<ClauseHead>,
    target: Lit,
    next_index: ClauseIndex,
}

impl<'a> ClausePartition {
    pub fn iter_watcher(&'a self, p: Lit) -> ClauseListIter<'a> {
        ClauseListIter {
            vec: &self.head,
            target: p,
            next_index: self.watcher[p.negate() as usize],
        }
    }
}

impl<'a> Iterator for ClauseListIter<'a> {
    type Item = ClauseIndex;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index == NULL_CLAUSE {
            None
        } else {
            let i = self.next_index as usize;
            let c = &self.vec[self.next_index as usize];
            self.next_index = c.next_watcher[(c.lit[0] != self.target) as usize];
            Some(i)
        }
    }
}

impl ConsistencyCheck for ClausePartition {
    fn check(&mut self, lit: Lit) -> bool {
        let max = self.head.len();
        for i in 2..self.watcher.len() {
            let mut p = self.watcher[(i as Lit).negate() as usize];
            let mut cnt = 0;
            while p != NULL_CLAUSE {
                cnt += 1;
                if max <= cnt {
                    println!(
                        "ConsistencyCheck::fail at {} during {}",
                        (i as Lit).int(),
                        lit.int()
                    );
                    return false;
                }
                p = self.head[p].next_watcher[(self.head[p].lit[0] != (i as Lit)) as usize];
            }
        }
        true
    }
}

pub trait ClauseManagement {
    fn add_clause(
        &mut self,
        config: &mut SolverConfiguration,
        eliminator: &mut Eliminator,
        vars: &mut [Var],
        v: &mut Vec<Lit>,
        lbd: usize,
        act: f64,
    ) -> ClauseId;
    fn remove_clause(&mut self, cid: ClauseId);
    fn change_clause_kind(
        &mut self,
        eliminator: &mut Eliminator,
        vars: &mut [Var],
        cid: ClauseId,
        kind: ClauseKind,
    );
    fn reduce(
        &mut self,
        eliminator: &mut Eliminator,
        stat: &mut [i64],
        vars: &mut [Var],
        next_reduction: &mut usize,
        lbd_temp: &mut [usize],
    );
}

impl ClauseManagement for [ClausePartition] {
    /// renamed from newLearntClause
    // Note: set lbd to 0 if you want to add the clause to Permanent.
    fn add_clause(
        &mut self,
        config: &mut SolverConfiguration,
        eliminator: &mut Eliminator,
        vars: &mut [Var],
        v: &mut Vec<Lit>,
        lbd: usize,
        act: f64,
    ) -> ClauseId {
        debug_assert!(1 < v.len());
        // let lbd = v.lbd(&self.vars, &mut self.lbd_temp);
        let mut i_max = 0;
        let mut lv_max = 0;
        // seek a literal with max level
        for (i, l) in v.iter().enumerate() {
            let vi = l.vi();
            let lv = vars[vi].level;
            if vars[vi].assign != BOTTOM && lv_max < lv {
                i_max = i;
                lv_max = lv;
            }
        }
        v.swap(1, i_max);
        let kind = if lbd == 0 && eliminator.use_elim {
            ClauseKind::Permanent
        } else if v.len() == 2 {
            ClauseKind::Binclause
        } else if (config.use_chan_seok && lbd <= config.co_lbd_bound) || lbd == 0 {
            ClauseKind::Permanent
        } else {
            ClauseKind::Removable
        };
        let cid = self[kind as usize].new_clause(&v, lbd);
        // self.bump_cid(cid);
        if cid.to_kind() == ClauseKind::Removable as usize {
            self[ClauseKind::Removable as usize].bump_activity(
                cid.to_index(),
                act,
                &mut config.cla_inc,
            );
        }
        let ch = clause_mut!(*self, cid);
        vars.attach_clause(cid, ch, false, eliminator);
        cid
    }
    /// 4. removeClause
    /// called from strengthen_clause, backward_subsumption_check, eliminate_var, substitute
    fn remove_clause(&mut self, cid: ClauseId) {
        // if clause_body!(self.cp, cid).get_flag(ClauseFlag::Dead) {
        //     panic!(
        //         "remove_clause Dead: {} {:#}{:#}",
        //         cid2fmt(cid),
        //         clause_head!(self.cp, cid),
        //         clause_body!(self.cp, cid)
        //     );
        // }
        clause_mut!(*self, cid).flag_on(ClauseFlag::Dead);
        let ch = clause!(*self, cid);
        let w0 = ch.lit[0].negate();
        let w1 = ch.lit[1].negate();
        debug_assert!(w0 != 0 && w1 != 0);
        debug_assert_ne!(w0, w1);
        self[cid.to_kind()].touched[w0 as usize] = true;
        self[cid.to_kind()].touched[w1 as usize] = true;
    }
    // This should be called at DL == 0.
    fn change_clause_kind(
        &mut self,
        eliminator: &mut Eliminator,
        vars: &mut [Var],
        cid: ClauseId,
        kind: ClauseKind,
    ) {
        // let dl = self.decision_level();
        // debug_assert_eq!(dl, 0);
        let ch = clause_mut!(*self, cid);
        if ch.get_flag(ClauseFlag::Dead) {
            return;
        }
        ch.flag_on(ClauseFlag::Dead);
        let mut vec = Vec::new();
        for x in &ch.lits {
            vec.push(*x);
        }
        let rank = ch.rank;
        let w0 = ch.lit[0].negate();
        let w1 = ch.lit[1].negate();
        self[kind as usize].new_clause(&vec, rank);
        self[cid.to_kind()].touched[w0 as usize] = true;
        self[cid.to_kind()].touched[w1 as usize] = true;
    }
    fn reduce(
        &mut self,
        eliminator: &mut Eliminator,
        stat: &mut [i64],
        vars: &mut [Var],
        next_reduction: &mut usize,
        lbd_temp: &mut [usize],
    ) {
        self[ClauseKind::Removable as usize].reset_lbd(vars, &mut lbd_temp[..]);
        let ClausePartition {
            ref mut head,
            ref mut touched,
            ref mut perm,
            ..
        } = &mut self[ClauseKind::Removable as usize];
        let mut nc = 1;
        for (i, b) in head.iter().enumerate().skip(1) {
            if !b.get_flag(ClauseFlag::Dead) && !vars.locked(b, ClauseKind::Removable.id_from(i)) {
                perm[nc] = i;
                nc += 1;
            }
        }
        perm[1..nc].sort_by(|&a, &b| head[a].cmp(&head[b]));
        let keep = nc / 2;
        if head[perm[keep]].rank <= 5 {
            *next_reduction += 1000;
        };
        for i in keep..nc {
            let ch = &mut head[perm[i]];
            if ch.get_flag(ClauseFlag::JustUsed) {
                ch.flag_off(ClauseFlag::JustUsed)
            } else {
                ch.flag_on(ClauseFlag::Dead);
                debug_assert!(ch.lit[0] != 0 && ch.lit[1] != 0);
                touched[ch.lit[0].negate() as usize] = true;
                touched[ch.lit[1].negate() as usize] = true;
            }
        }
        self[ClauseKind::Removable as usize].garbage_collect(vars, eliminator);
        *next_reduction += DB_INC_SIZE;
        stat[Stat::Reduction as usize] += 1;
    }
}
