use clause::Clause;
use clause::ClauseIdIndexEncoding;
use clause::ClauseIndex;
use clause::ClauseKind;
use clause::ClauseFlag;
use clause::ClauseManagement;
use solver::{Solver, Stat};
use solver_analyze::CDCL;
use solver_rollback::Restart;
use std::cmp::max;
use types::*;
use var::Satisfiability;
use var_manage::VarSelect;

pub trait SolveSAT {
    /// returns `true` for SAT, `false` for UNSAT.
    fn search(&mut self) -> bool;
    fn propagate(&mut self) -> ClauseId;
    fn enqueue(&mut self, l: Lit, cid: ClauseId) -> bool;
}

impl SolveSAT for Solver {
    fn propagate(&mut self) -> ClauseId {
        while self.q_head < self.trail.len() {
            let p: usize = self.trail[self.q_head] as usize;
            let false_lit = (p as Lit).negate();
            self.q_head += 1;
            self.stats[Stat::NumOfPropagation as usize] += 1;
            let kinds = [
                ClauseKind::Binclause,
                ClauseKind::Permanent,
                ClauseKind::Removable,
            ];
            let mut ci: ClauseIndex;
            for kind in &kinds {
                unsafe {
                    let clauses = &mut self.cp[*kind as usize].clauses[..] as *mut [Clause];
                    let watcher = &mut self.cp[*kind as usize].watcher[..] as *mut [ClauseIndex];
                    ci = (*watcher)[p];
                    let mut tail = &mut (*watcher)[p] as *mut usize;
                    *tail = NULL_CLAUSE;
                    'next_clause: while ci != NULL_CLAUSE {
                        let c = &mut (*clauses)[ci] as *mut Clause;
                        if (*c).lit[0] == false_lit {
                            (*c).lit.swap(0, 1); // now my index is 1, others is 0.
                            (*c).next_watcher.swap(0, 1);
                        }
                        ci = (*c).next_watcher[1];
                        // let next = (*c).next_watcher[1];
                        let other_value = self.assigned((*c).lit[0]);
                        if other_value != LTRUE {
                            for (k, lk) in (*c).lits.iter().enumerate() {
                                // below is equivalent to 'self.assigned(lk) != LFALSE'
                                if (((lk & 1) as u8) ^ self.vars[lk.vi()].assign) != 0 {
                                    let alt = &mut (*watcher)[lk.negate() as usize];
                                    (*c).next_watcher[1] = *alt;
                                    *alt = (*c).index;
                                    (*c).lit[1] = *lk;
                                    (*c).lits[k] = false_lit; // WARN: update this lastly (needed by enuremate)
                                    continue 'next_clause;
                                }
                            }
                            if other_value == LFALSE {
                                *tail = (*c).index;
                                return kind.id_from((*c).index);
                            } else {
                                self.uncheck_enqueue((*c).lit[0], kind.id_from((*c).index));
                            }
                        }
                        let watch = (*watcher)[p];
                        if watch == 0 {
                            tail = &mut (*c).next_watcher[1];
                        }
                        (*c).next_watcher[1] = watch;
                        (*watcher)[p] = (*c).index;
                    }
                }
            }
        }
        NULL_CLAUSE
    }
    fn search(&mut self) -> bool {
        // self.dump("search");
        let root_lv = self.root_level;
        loop {
            // self.dump("calling propagate");
            self.stats[Stat::NumOfPropagation as usize] += 1;
            let ci = self.propagate();
            let d = self.decision_level();
            // self.dump(format!("search called propagate and it returned {:?} at {}", ret, d));
            if ci == NULL_CLAUSE {
                // println!(" search loop enters a new level");
                let na = self.num_assigns();
                if na == self.num_vars {
                    return true;
                }
                self.force_restart();
                if d == 0 && self.num_solved_vars < na {
                    self.simplify_database();
                    self.num_solved_vars = na;
                    self.rebuild_vh();
                }
                if self.trail.len() <= self.q_head {
                    let vi = self.select_var();
                    debug_assert_ne!(vi, 0);
                    let p = self.vars[vi].phase;
                    self.uncheck_assume(vi.lit(p));
                }
            } else {
                self.stats[Stat::NumOfBackjump as usize] += 1;
                if d == self.root_level {
                    self.analyze_final(ci, false);
                    return false;
                } else {
                    // self.dump(" before analyze");
                    let backtrack_level = self.analyze(ci);
                    self.cancel_until(max(backtrack_level as usize, root_lv));
                    let lbd;
                    if self.an_learnt_lits.len() == 1 {
                        let l = self.an_learnt_lits[0];
                        self.uncheck_enqueue(l, NULL_CLAUSE);
                        lbd = 1;
                    } else {
                        unsafe {
                            let v = &mut self.an_learnt_lits as *mut Vec<Lit>;
                            lbd = self.add_learnt(&mut *v);
                        }
                    }
                    self.decay_var_activity();
                    self.decay_cla_activity();
                    // glucose reduction
                    let conflicts = self.stats[Stat::NumOfBackjump as usize] as usize;
                    if self.cur_restart * self.next_reduction <= conflicts {
                        self.cur_restart =
                            ((conflicts as f64) / (self.next_reduction as f64)) as usize + 1;
                        self.reduce_watchers();
                    }
                    self.block_restart(lbd, d);
                }
                // Since the conflict path pushes a new literal to trail, we don't need to pick up a literal here.
            }
        }
    }
    /// This function touches:
    ///  - vars
    ///  - trail
    fn enqueue(&mut self, l: Lit, cid: ClauseId) -> bool {
        // println!("enqueue: {} by {}", l.int(), cid);
        let sig = l.lbool();
        let val = self.vars[l.vi()].assign;
        if val == BOTTOM {
            {
                let dl = self.decision_level();
                let v = &mut self.vars[l.vi()];
                v.assign = sig;
                v.level = dl;
                v.reason = cid;
                mref!(self.cp, cid).set_flag(ClauseFlag::Locked, true);
            }
            // println!(
            //     "implication {} by {} {}",
            //     l.int(),
            //     cid.to_kind(),
            //     cid.to_index()
            // );
            self.trail.push(l);
            true
        } else {
            val == sig
        }
    }
}

impl Solver {
    /// This function touches:
    ///  - vars
    ///  - trail
    ///  - trail_lim
    pub fn uncheck_enqueue(&mut self, l: Lit, cid: ClauseId) -> () {
        // if ci == NULL_CLAUSE {
        //     println!("uncheck_enqueue decide: {}", l.int());
        // } else {
        //     println!("uncheck_enqueue imply: {} by {}", l.int(), ci);
        // }
        debug_assert!(l != 0, "Null literal is about to be equeued");
        let dl = self.decision_level();
        let v = &mut self.vars[l.vi()];
        v.assign = l.lbool();
        v.level = dl;
        v.reason = cid;
        mref!(self.cp, cid).set_flag(ClauseFlag::Locked, true);
        // if 0 < cid {
        //     println!(
        //         "::uncheck_enqueue of {} by {}::{}",
        //         l.int(),
        //         cid.to_kind(),
        //         cid.to_index(),
        //     );
        // }
        self.trail.push(l);
    }
    pub fn uncheck_assume(&mut self, l: Lit) -> () {
        self.trail_lim.push(self.trail.len());
        // println!("::decision {}", l.int());
        self.uncheck_enqueue(l, NULL_CLAUSE);
    }
}
