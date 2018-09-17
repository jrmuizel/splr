use clause::ClauseIdIndexEncoding;
use clause::ClauseFlag;
use solver::{Solver, Stat};
use types::*;
use var::VarOrdering;

pub trait Restart {
    fn cancel_until(&mut self, lv: usize) -> ();
    fn force_restart(&mut self) -> ();
    fn block_restart(&mut self, lbd: usize, clv: usize) -> ();
}

/// for block restart based on average assigments: 1.40
const R: f64 = 1.02;
/// for force restart based on average LBD of newly generated clauses: 1.15
const K: f64 = 1.28;

impl Restart for Solver {
    /// This function touches:
    ///  - trail
    ///  - trail_lim
    ///  - vars
    ///  - q_head
    ///  - var_order
    fn cancel_until(&mut self, lv: usize) -> () {
        if self.decision_level() <= lv {
            return;
        }
        let lim = self.trail_lim[lv];
        for l in &self.trail[lim..] {
            let vi = l.vi();
            {
                let v = &mut self.vars[vi];
                v.phase = v.assign;
                v.assign = BOTTOM;
                if 0 < v.reason {
                    self.cp[v.reason.to_kind()].clauses[v.reason.to_index()].set_flag(ClauseFlag::Locked, false);
                }
                v.reason = NULL_CLAUSE;
            }
            self.var_order.insert(&self.vars, vi);
        }
        self.trail.truncate(lim); // FIXME
        self.trail_lim.truncate(lv);
        self.q_head = lim;
    }
    /// called after no conflict propagation
    fn force_restart(&mut self) -> () {
        let count = self.stats[Stat::NumOfBackjump as usize] as u64;
        let e_lbd = self.ema_lbd.get();
        let should_force = K < e_lbd;
        if self.next_restart < count && should_force {
            self.next_restart = count + 50;
            self.stats[Stat::NumOfRestart as usize] += 1;
            let rl = self.root_level;
            self.cancel_until(rl);
            // println!(" - {:.2} {:.2}", cnt, e_lbd);
        }
    }
    /// called after conflict resolution
    fn block_restart(&mut self, lbd: usize, clv: usize) -> () {
        let count = self.stats[Stat::NumOfBackjump as usize] as u64;
        let nas = self.num_assigns() as f64;
        let b_l = self.decision_level() as f64;
        self.ema_asg.update(nas);
        let e_asg = self.ema_asg.get();
        self.ema_lbd.update(lbd as f64);
        let _e_lbd = self.ema_lbd.get();
        self.c_lvl.update(clv as f64);
        self.b_lvl.update(b_l);
        let should_block = R < e_asg;
        if 100 < count && self.next_restart <= count && should_block {
            self.next_restart = count + 50;
            self.stats[Stat::NumOfBlockRestart as usize] += 1;
            // println!("blocking {:.2} {:.2}", e_asg, self.stats[Stat::NumOfBlockRestart as usize]);
        }
    }
}
