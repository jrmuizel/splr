use clause::*;
use solver::*;
use std::cmp::max;
use std::usize::MAX;
use types::*;

const LEVEL_BITMAP_SIZE: usize = 256;

impl Solver {
    /// renamed from newLearntClause
    pub fn add_learnt(&mut self, v: Vec<Lit>) -> usize {
        let k = v.len();
        if k == 1 {
            self.unsafe_enqueue(v[0], NULL_CLAUSE);
            return 1;
        }
        let mut c = Clause::new(true, v);
        let mut i_max = 0;
        let mut lv_max = 0;
        // seek a literal with max level
        for (i, l) in c.lits.iter().enumerate() {
            let vi = l.vi();
            let lv = self.vars[vi].level;
            if self.vars[vi].assign != BOTTOM && lv_max < lv {
                i_max = i;
                lv_max = lv;
            }
        }
        c.lits.swap(1, i_max);
        let l0 = c.lits[0];
        let lbd = self.lbd_of(&c.lits);
        c.rank = lbd;
        let ci = self.inject(c);
        self.bump_ci(ci);
        self.unsafe_enqueue(l0, ci);
        debug_assert!(1 < lbd, "lbd of a new clause is too small");
        lbd
    }
    // adapt delayed update of watches
    fn propagate(&mut self) -> Option<ClauseIndex> {
        // println!("> propagate at {}", self.decision_level());
        loop {
            if self.trail.len() <= self.q_head {
                // println!("<  propagate done");
                return None;
            }
            let p: Lit = self.trail[self.q_head];
            let p_usize: usize = p as usize;
            self.q_head += 1;
            self.stats[StatIndex::NumOfPropagation as usize] += 1;
            {
                let wl = self.watches[p_usize].len();
                let false_lit = p.negate();
                'next_clause: for mut wi in 0..wl {
                    // println!(" next_clause: {}", wi);
                    let Watch {
                        other: blocker,
                        by: ci,
                        to,
                    } = self.watches[p_usize][wi];
                    debug_assert!(blocker != to, "An inconsistent Watch");
                    // We use `Watch.to` to keep the literal which is the destination of propagation.
                    let bv = if blocker == 0 {
                        LFALSE
                    } else {
                        self.assigned(blocker)
                    };
                    if bv == LTRUE {
                        self.watches[p_usize][wi].to = p;
                        continue 'next_clause;
                    }
                    unsafe {
                        let c = &mut self.clauses[ci] as *mut Clause;
                        debug_assert!(ci == (*c).index, "A clause has an inconsistent index");
                        if !(false_lit == (*c).lits[0] || false_lit == (*c).lits[1]) {
                            panic!(
                                "Watch literals error by {} ({}, {}) for propagate({})",
                                (*c),
                                (*c).lits[0].int(),
                                (*c).lits[1].int(),
                                p.int()
                            );
                        }
                        // Place the false literal at lits[1].
                        // And the last literal in unit clause will be at lits[0].
                        if (*c).lits[0] == false_lit {
                            (*c).lits.swap(0, 1);
                        }
                        let first = (*c).lits[0];
                        debug_assert!(
                            false_lit == (*c).lits[1],
                            "Inconsisitent watch registration"
                        );
                        let fv = self.assigned(first);
                        if fv == LTRUE {
                            // Satisfied by the other watch.
                            // Update watch with `other`, the cached literal
                            self.watches[p_usize][wi].to = p;
                            continue 'next_clause;
                        }
                        self.watches[p_usize][wi].other = first;
                        for k in 2..(*c).lits.len() {
                            let lk = (*c).lits[k];
                            if self.assigned(lk) != LFALSE {
                                (*c).tmp = k;
                                // Update the watch
                                self.watches[p_usize][wi].to = lk.negate();
                                continue 'next_clause;
                            }
                        }
                        if fv == LFALSE {
                            // println!("  found a conflict variable {} by {}", first.vi(), (*c));
                            return Some(ci);
                        } else {
                            // println!("  unit propagation {} by {}", first.int(), (*c));
                            (*c).tmp = 1;
                            self.watches[p_usize][wi].to = p;
                            debug_assert!(
                                first == (*c).lits[0],
                                "Inconsisitent clause propagation update"
                            );
                            self.unsafe_enqueue(first, ci);
                        }
                    }
                }
                // No conflict: so let's move them!
                // use watches[0] to keep watches that don't move anywhere, temporally.
                // println!("  update watches");
                self.watches[0].clear();
                loop {
                    // println!("   remain: {}", self.watches[p_usize].len());
                    match self.watches[p_usize].pop() {
                        Some(w) => {
                            debug_assert!(w.to != 0, "Invalid Watch.to found");
                            if w.to == p {
                                self.watches[0].push(w)
                            } else {
                                unsafe {
                                    let c = &mut self.clauses[w.by] as *mut Clause;
                                    let k = (*c).tmp as usize;
                                    // println!("moving {} with {} to {}", (*c), k, w.to.int());
                                    (*c).lits.swap(1, k);
                                }
                                // println!("move {} to {}", w.by, w.to.int());
                                self.watches[w.to as usize].push(w);
                            }
                        }
                        None => break,
                    }
                }
                debug_assert!(
                    self.watches[p_usize].is_empty(),
                    "propagate failed to clear watches"
                );
                loop {
                    match self.watches[0].pop() {
                        Some(w) => {
                            // println!("  loop back by {} to {}", w.by, w.to.int());
                            debug_assert!(w.to == p, "inconsistent propagation");
                            self.watches[p_usize].push(w);
                        }
                        None => break,
                    }
                }
            }
        }
    }
    fn analyze(&mut self, confl: ClauseIndex) -> (usize, Vec<Lit>) {
        for mut l in &mut self.an_seen {
            *l = 0;
        }
        // self.dump("analyze");
        self.an_learnt_lits.clear();
        self.an_learnt_lits.push(0);
        let dl = self.decision_level();
        let mut ci = confl;
        let mut p = NULL_LIT;
        let mut ti = self.trail.len() - 1; // trail index
        let mut b: usize = 0; // backtrack level
        let mut path_cnt = 0;
        loop {
            unsafe {
                debug_assert!(ci != NULL_CLAUSE, "Null clause found in analzye");
                let c = &mut self.clauses[ci] as *mut Clause;
                // println!("  analyze.loop {}", (*c));
                let d = (*c).rank;
                if 0 < d {
                    self.bump_ci(ci);
                    let nblevel = self.lbd_of(&(*c).lits);
                    if 2 < d && nblevel + 1 < d {
                        (*c).rank = nblevel;
                    }
                }
                // println!("{}を対応", (*c));
                for q in &(*c).lits[if p == NULL_LIT { 0 } else { 1 }..] {
                    let vi = q.vi();
                    let l = self.vars[vi].level;
                    debug_assert!(
                        self.vars[vi].assign != BOTTOM,
                        "Unbound literal found in analyze"
                    );
                    if self.an_seen[vi] == 0 && 0 < l {
                        self.bump_vi(vi);
                        self.an_seen[vi] = 1;
                        if dl <= l {
                            // println!(
                            //     "{} はレベル{}なのでフラグを立てる",
                            //     q.int(),
                            //     l
                            // );
                            path_cnt += 1;
                            if 0 < self.vars[vi].reason {
                                self.an_last_dl.push(*q);
                            }
                        } else {
                            // println!("{} はレベル{}なので採用", q.int(), l);
                            self.an_learnt_lits.push(*q);
                            b = max(b, l);
                        }
                    } else {
                        // println!("{} はもうフラグが立っているかグラウンドしている{}ので無視", q.int(), l);
                    }
                }
                // set the index of the next literal to ti
                while self.an_seen[self.trail[ti].vi()] == 0 {
                    // println!(
                    //     "{} はフラグが立ってないので飛ばす",
                    //     self.trail[ti].int()
                    // );
                    ti -= 1;
                }
                p = self.trail[ti];
                {
                    let next_vi: VarIndex = p.vi();
                    ci = self.vars[next_vi].reason;
                    // println!("{} にフラグが立っている。この時path数は{}, そのreason {}を探索", next_vi, path_cnt - 1, ci);
                    self.an_seen[next_vi] = 0;
                }
                path_cnt -= 1;
                if path_cnt <= 0 {
                    break;
                }
                ti -= 1;
            }
        }
        self.an_learnt_lits[0] = p.negate();
        // println!(
        //     "最後に{}を採用して{:?}",
        //     p.negate().int(),
        //     self.an_learnt_lits
        //         .iter()
        //         .map(|l| l.int())
        //         .collect::<Vec<i32>>()
        // );
        let level_to_return: usize = b;
        // simlpify phase
        let n = self.an_learnt_lits.len();
        let l0 = self.an_learnt_lits[0];
        self.an_stack.clear();
        self.an_to_clear.clear();
        self.an_to_clear.push(l0);
        let mut level_map = [false; LEVEL_BITMAP_SIZE];
        for i in 1..n {
            let l = self.an_learnt_lits[i];
            self.an_to_clear.push(l);
            level_map[(self.vars[l.vi()].level as usize) % LEVEL_BITMAP_SIZE] = true;
        }
        // println!("  analyze.loop 4 n = {}", n);
        let mut i = 1;
        let mut j = 1;
        loop {
            // println!("  analyze.loop for simplify {} {}", i, n);
            if i == n {
                self.an_learnt_lits.truncate(j + 1);
                break;
            }
            let l = self.an_learnt_lits[i];
            if self.vars[l.vi()].reason == NULL_CLAUSE {
                self.an_learnt_lits[j] = l;
                j += 1;
            } else if !self.analyze_removable(l, &level_map) {
                self.an_learnt_lits[j] = l;
                j += 1;
            }
            i += 1;
        }
        self.an_learnt_lits.truncate(j);
        // glucose heuristics
        // println!("  analyze.loop 5");
        let r = self.an_learnt_lits.len();
        for i in 0..self.an_last_dl.len() {
            let l = self.an_last_dl[i];
            let vi = l.vi();
            let ci = self.vars[vi].reason;
            let len = self.clauses[ci].lits.len();
            if r < len {
                self.bump_vi(vi);
            }
        }
        self.an_last_dl.clear();
        for l in &self.an_to_clear {
            self.an_seen[l.vi()] = 0;
        }
        // println!(
        //     "new learnt: {:?}",
        //     self.an_learnt_lits
        //         .iter()
        //         .map(|l| l.int())
        //         .collect::<Vec<i32>>()
        // );
        // println!("  analyze terminated");
        (level_to_return, self.an_learnt_lits.clone())
    }
    fn analyze_removable(&mut self, l: Lit, level_map: &[bool]) -> bool {
        self.an_stack.clear();
        self.an_stack.push(l);
        let top1 = self.an_to_clear.len();
        loop {
            // println!("analyze_removable.loop {:?}", self.an_stack);
            if self.an_stack.is_empty() {
                return true;
            }
            let sl = self.an_stack.pop().unwrap();
            let ci = self.vars[sl.vi()].reason;
            let c = &self.clauses[ci] as *const Clause;
            unsafe {
                for q in &(*c).lits {
                    let vi = q.vi();
                    let lv = self.vars[vi].level % LEVEL_BITMAP_SIZE;
                    if self.an_seen[vi] != 1 && lv != 0 {
                        if self.vars[vi].reason != NULL_CLAUSE && level_map[lv] {
                            self.an_seen[vi] = 1;
                            self.an_stack.push(*q);
                            self.an_to_clear.push(*q);
                        } else {
                            let top2 = self.an_to_clear.len();
                            for _ in top1..top2 {
                                self.an_seen[self.an_to_clear.pop().unwrap().vi()] = 0;
                            }
                            return false;
                        }
                    }
                }
            }
        }
    }
    fn analyze_final(&mut self, ci: ClauseIndex, skip_first: bool) -> () {
        self.conflicts.clear();
        if self.root_level != 0 {
            unsafe {
                let c = &self.clauses[ci] as *const Clause;
                for l in &(*c).lits[(if skip_first { 1 } else { 0 })..] {
                    let vi = l.vi();
                    if 0 < self.vars[vi].level {
                        self.an_seen[vi] = 1;
                    }
                }
            }
            let tl0 = self.trail_lim[0];
            let start = if self.trail_lim.len() <= self.root_level {
                self.trail.len()
            } else {
                self.trail_lim[self.root_level]
            };
            for i in (tl0..start).rev() {
                let l: Lit = self.trail[i];
                let vi = l.vi();
                if self.an_seen[vi] == 1 {
                    if self.vars[vi].reason == NULL_CLAUSE {
                        self.conflicts.push(l.negate());
                    } else {
                        let c = &self.clauses[ci] as *const Clause;
                        unsafe {
                            for l in &(*c).lits[1..] {
                                let vi = l.vi();
                                if 0 < self.vars[vi].level {
                                    self.an_seen[vi] = 1;
                                }
                            }
                        }
                    }
                }
                self.an_seen[vi] = 0;
            }
        }
    }
    /// returns:
    /// - true for SAT
    /// - false for UNSAT
    fn search(&mut self) -> bool {
        // self.dump("search");
        let delta: f64 = if 30000 < self.num_vars {
            (self.num_vars as f64).sqrt()
        } else {
            200.0
        };
        let root_lv = self.root_level;
        let mut to_restart = false;
        loop {
            // self.dump("calling propagate");
            let ret = self.propagate();
            let d = self.decision_level();
            // self.dump(format!("search called propagate and it returned {:?} at {}", ret, d));
            match ret {
                Some(ci) => {
                    self.stats[StatIndex::NumOfBackjump as usize] += 1;
                    if d == self.root_level {
                        self.analyze_final(ci, false);
                        return false;
                    } else {
                        // self.dump(" before analyze");
                        let (backtrack_level, v) = self.analyze(ci);
                        // println!(
                        //     " conflict analyzed {:?}",
                        //     v.iter().map(|l| l.int()).collect::<Vec<i32>>()
                        // );
                        self.cancel_until(max(backtrack_level as usize, root_lv));
                        // println!(" backtracked to {}", backtrack_level);
                        let lbd = self.add_learnt(v);
                        self.decay_var_activity();
                        self.decay_cla_activity();
                        self.learnt_size_cnt -= 1;
                        if self.learnt_size_cnt == 0 {
                            let t_ = 1.5 * self.learnt_size_adj;
                            self.learnt_size_adj = t_;
                            self.learnt_size_cnt = t_ as u64;
                            self.max_learnts += delta;
                            to_restart = self.should_restart(lbd, d);
                            continue;
                        }
                    }
                }
                None => {
                    // println!(" search loop enter a new level");
                    let na = self.num_assigns();
                    if (self.max_learnts as usize) + na < self.clauses.len() - self.fixed_len {
                        self.reduce_database(false);
                    } else if d == 0 {
                        self.reduce_database(true);
                    }
                    if na == self.num_vars {
                        return true;
                    } else if to_restart {
                        self.cancel_until(root_lv);
                        to_restart = false;
                    } else {
                        let vi = self.select_var();
                        // println!(" search loop find a new decision var");
                        debug_assert!(vi != 0, "No more decision var");
                        // println!(" {:?}", self.var_order);
                        if vi != 0 {
                            let p = self.vars[vi].phase;
                            self.unsafe_assume(vi.lit(p));
                        }
                    }
                }
            }
        }
    }
    pub fn solve(&mut self) -> SolverResult {
        // TODO deal with assumptons
        // s.root_level = 0;
        match self.search() {
            _ if self.ok == false => {
                self.cancel_until(0);
                Err(SolverException::InternalInconsistent)
            }
            true => {
                let mut result = Vec::new();
                for vi in 1..self.num_vars + 1 {
                    match self.vars[vi].assign {
                        LTRUE => result.push(vi as i32),
                        LFALSE => result.push(0 - vi as i32),
                        _ => (),
                    }
                }
                self.cancel_until(0);
                Ok(Certificate::SAT(result))
            }
            false => {
                self.cancel_until(0);
                let mut v = Vec::new();
                for l in &self.conflicts {
                    v.push(l.int());
                }
                Ok(Certificate::UNSAT(v))
            }
        }
    }
    fn unsafe_enqueue(&mut self, l: Lit, ci: ClauseIndex) -> () {
        // if ci == NULL_CLAUSE {
        //     println!("unsafe_enqueue decide: {}", l.int());
        // } else {
        //     println!("unsafe_enqueue imply: {} by {}", l.int(), ci);
        // }
        debug_assert!(l != 0, "Null literal is about to be equeued");
        let dl = self.decision_level();
        let v = &mut self.vars[l.vi()];
        v.assign = l.lbool();
        v.level = dl;
        v.reason = ci;
        self.trail.push(l)
    }
    fn unsafe_assume(&mut self, l: Lit) -> () {
        let len = self.trail.len();
        self.trail_lim.push(len);
        self.unsafe_enqueue(l, NULL_CLAUSE);
    }
}

/// reduce_database
impl Solver {
    pub fn reduce_database(&mut self, simplify: bool) -> () {
        debug_assert!(
            !simplify || self.decision_level() == 0,
            "wrong invocation of reduce_database"
        );
        let end = self.clauses.len();
        let new_end = self.sort_clauses(simplify);
        self.rebuild_reason();
        self.clauses.truncate(new_end);
        if simplify {
            let mut fixed = 0;
            for c in &self.clauses {
                if c.rank == 0 {
                    fixed += 1;
                }
            }
            self.fixed_len = fixed;
        }
        self.rebuild_watches();
        if simplify {
            println!(
                "< rebuild_database with simplification {} ({})  => {} / {:.1}",
                end, self.fixed_len, new_end, self.max_learnts
            );
        } else {
            println!(
                "< rebuild_database {} ({}) => {} / {:.1}",
                end, self.fixed_len, new_end, self.max_learnts
            );
        }
    }
    /// Note: this function changes self.clause_permutation.
    fn sort_clauses(&mut self, simplify: bool) -> usize {
        let start = if simplify { 1 } else { self.fixed_len };
        let nc = self.clauses.len();
        let mut requires = 0;
        if self.clause_permutation.len() < nc {
            unsafe {
                self.clause_permutation.reserve(nc + 1);
                self.clause_permutation.set_len(nc + 1);
            }
        }
        // reinitialize the permutation table.
        for (i, x) in self.clause_permutation.iter_mut().enumerate() {
            *x = i;
        }
        // reset the 'tmp' field
        for c in &mut self.clauses[start..] {
            (*c).tmp = 100;
        }
        // mark clauses that used as reasons
        for v in &self.vars[1..] {
            self.clauses[v.reason].tmp = 0;
        }
        // set key
        let ac = 0.1 * self.cla_inc / (nc as f64);
        for ci in start..self.clauses.len() {
            unsafe {
                let mut c = &mut self.clauses[ci] as *mut Clause;
                if (*c).tmp == 0 {
                    requires += 1;
                } else if simplify && self.satisfies(&*c) {
                    (*c).tmp = MAX;
                } else if (*c).rank == 0 {
                    (*c).tmp = 0;
                    requires += 1;
                } else {
                    requires += (*c).set_sort_key(ac);
                }
            }
        }
        // sort the range
        {
            let vec = &mut self.clauses[start..];
            vec.sort_by_key(|c| c.tmp);
        }
        // update permutation table.
        for i in 1..nc {
            let old = self.clauses[i].index;
            debug_assert!(0 < old, "0 index");
            self.clause_permutation[old] = i;
            self.clauses[i].index = i;
        }
        debug_assert!(self.clauses[0].index == 0, "NULL moved.");
        if simplify {
            start + requires + (nc - start) / 4
        } else {
            start + max(requires, (nc - start) / 2)
        }
    }
    fn rebuild_reason(&mut self) -> () {
        let len = &self.clauses.len();
        let new_clause = &self.clause_permutation;
        // println!("perm {:?}", perm);
        for v in &mut self.vars[1..] {
            let ci = v.reason;
            v.reason = new_clause[ci];
            if len < &new_clause[ci] {
                panic!("strange index for permutation {} {}", ci, len);
            }
        }
    }
    fn rebuild_watches(&mut self) -> () {
        // Firstly, clear everything.
        // for i in 1..self.watches.len() {
        //     let w = &mut self.watches[i];
        //     w.clear();
        // }
        for w in &mut self.watches {
            w.clear();
        }
        // assert_eq!(self.clauses[0].index, 0);
        for c in &self.clauses[1..] {
            if 2 <= c.lits.len() {
                push_to_watch(&mut self.watches, c.index, c.lits[0], c.lits[1]);
            }
        }
        // self.dump(&format!("rebuild {}", self.learnts.len()));
    }
}
