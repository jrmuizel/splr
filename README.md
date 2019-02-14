Splr -- SAT Solver for Propositional Logic in Rust
----

## Features

- Splr is a pure [Rust](https://www.rust-lang.org)ic SAT solver, based on [Glucose 4.1](https://www.labri.fr/perso/lsimon/glucose/).
- It adopts many ideas in modern SAT solvers like:
  - Glucose-like dynamic blocking/forcing restarts based on [EMAs](https://arxiv.org/abs/1506.08905)
  - heuristics adaptation
  - pre/in-process simplification based on clause subsumption and variable elimination

## Install

Just clone me, and `cargo install`.

Two executables will be installed:

- `splr` -- SAT solver
- `dmcr` -- A model checker to verify assignments which are generated by `splr`.

## Usage

Splr is a standalone program, taking an CNF file. The result will be saved to a file.

```
$ splr tests/sample.cnf
sample.cnf                         250,1065 |time:     0.35, Mode:  Initial
 #conflict:      19242, #decision:        22518, #propagate:         866681
  Assignment|#rem:      243, #fix:        1, #elm:        6, prg%:   2.8000
 Clause Kind|Remv:    11255, LBD2:       61, Binc:        0, Perm:     1056
     Restart|#BLK:      276, #RST:        0, eASG:   0.4211, eLBD:   1.0312
   Conflicts|aLBD:     9.37, bjmp:     9.21, cnfl:    11.66 |blkR:   1.4000
   Clause DB|#rdc:        4, #sce:        2, #exe:        1 |frcK:   0.6100
SATISFIABLE: sample.cnf. The answer was dumped to .ans_sample.cnf.

$ cat .ans_sample.cnf
c An assignment generated by splr-0.1.0 for SATISFIABLE tests/sample.cnf
c
c sample.cnf                         , v:     250, c:    1065, time:     0.35
c  #conflict:      19242, #decision:        22518, #propagate:         866681
c   Assignment|#rem:      243, #fix:        1, #elm:        6, prg%:   2.8000
c  Clause Kind|Remv:    11255, LBD2:       61, Binc:        0, Perm:     1056
c      Restart|#BLK:      276, #RST:        0, eASG:   0.4211, eLBD:   1.0312
c    Conflicts|aLBD:     9.37, bjmp:     9.21, cnfl:    11.66 |blkR:   1.4000
c    Clause DB|#rdc:        4, #sce:        2, #exe:        1 |frcK:   0.6100
c
1 2 3 4 -5 6 7 -8 -9 10 -11 -12 -13 -14 15 16 -17 18 -19 -20 -21 -22 ... 0

$ dmcr tests/sample.cnf
Valid assignment for tests/sample.cnf found in .ans_sample.cnf.
```

## Correctness

While Splr comes with **ABSOLUTELY NO WARRANTY**, Splr version 0.1.0 (splr-0.1.0) was verified with the following problems:

* The first 100 satisfiable 3-SAT problems from
  [SATLIB](https://www.cs.ubc.ca/~hoos/SATLIB/benchm.html), [250 variables 3-SAT](https://www.cs.ubc.ca/~hoos/SATLIB/Benchmarks/SAT/RND3SAT/uf250-1065.tar.gz): passed
* [SAT Competition 2017](https://baldur.iti.kit.edu/sat-competition-2017/index.php?cat=tracks), [Main track](https://baldur.iti.kit.edu/sat-competition-2017/benchmarks/Main.zip): with a 2000 sec timeout, Splr-0.1.0 solved:
  * 72 satisfiable problems: all the solutions are correct (checked with dmcr).
  * 51 unsatisfiable problems: Lingeling or Glucose completely returns the same result.


----
2019, Shuji Narazaki
