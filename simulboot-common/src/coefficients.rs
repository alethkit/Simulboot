//! The v1 coefficient semiring `R` and its ordering.
//!
//! v1 grades every surface with a coefficient drawn from
//!
//! ```text
//! R = Interval^n  ×  (Confidentiality × Integrity)  ×  ℕ
//! ```
//!
//! (timing multi-interval × IFC label × linearity count), exactly as
//! `docs/persistence/session-coefficients-v1.xsd` specifies. v0 has none of this;
//! it behaves as if every surface sits at the **top** of `R` — the *most
//! permissive* value: no timing guarantee, no security restriction, unbounded
//! copying.
//!
//! # Order
//!
//! `R` is ordered so that **smaller = more constrained** and `⊤` (top) is the
//! greatest, least-constrained element. This is the order the Galois connection
//! (`crate::galois`) is built over, so the orientation matters:
//!
//! * timing: a narrower interval is *smaller*; `[-∞, +∞]` is the top (no
//!   guarantee). `a ⊑ b` iff `a`'s interval is contained in `b`'s.
//! * confidentiality: `secret ⊑ restricted ⊑ public`; `public` is top.
//! * integrity: `trusted ⊑ endorsed ⊑ untrusted`; `untrusted` is top.
//! * linearity: `Finite(n) ⊑ Finite(m)` iff `n ≤ m`, and every finite count
//!   `⊑ omega`; `omega` is top.
//!
//! [`Coefficient::top`] is the join-irreducible maximum: `x ⊑ ⊤` for all `x`.
//! That fact is what makes `γ` (which embeds at top) the upper adjoint of `α`.

/// One endpoint of a timing interval. `lo`/`hi` are nanoseconds, or unbounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bound {
    /// `-∞` — serialised `-INF`.
    NegInf,
    /// A finite delay in nanoseconds.
    Finite(f64),
    /// `+∞` — serialised `+INF`.
    PosInf,
}

impl Bound {
    /// A total comparison key. `NegInf < Finite(_) < PosInf`; finite values
    /// compare numerically. Inputs are never `NaN` (the parser rejects it).
    fn key(&self) -> (i8, f64) {
        match self {
            Bound::NegInf => (-1, 0.0),
            Bound::Finite(x) => (0, *x),
            Bound::PosInf => (1, 0.0),
        }
    }

    /// `self ≤ other` in the extended-reals order.
    fn le(&self, other: &Bound) -> bool {
        let (ta, va) = self.key();
        let (tb, vb) = other.key();
        if ta != tb {
            ta < tb
        } else {
            va <= vb // same tag; finite values (never NaN) compare numerically
        }
    }

    /// Canonical string form: `-INF`, `+INF`, or the shortest round-tripping
    /// decimal.
    pub fn to_str(&self) -> String {
        match self {
            Bound::NegInf => "-INF".to_string(),
            Bound::PosInf => "+INF".to_string(),
            Bound::Finite(v) => fmt_f64(*v),
        }
    }

    /// Parse a bound from its string form (`-INF` / `+INF` / decimal).
    pub fn parse(s: &str) -> Option<Bound> {
        match s.trim() {
            "-INF" => Some(Bound::NegInf),
            "+INF" | "INF" => Some(Bound::PosInf),
            other => other.parse::<f64>().ok().filter(|v| !v.is_nan()).map(Bound::Finite),
        }
    }
}

/// One stability interval (one delay path through the compositor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub lo: Bound,
    pub hi: Bound,
}

impl Interval {
    /// The unbounded interval `[-∞, +∞]` — the timing top (no guarantee).
    pub fn unbounded() -> Interval {
        Interval { lo: Bound::NegInf, hi: Bound::PosInf }
    }

    /// `self ⊑ other`: `self` is contained in `other` (narrower = smaller).
    pub fn contained_in(&self, other: &Interval) -> bool {
        other.lo.le(&self.lo) && self.hi.le(&other.hi)
    }
}

/// IFC confidentiality label. Top (most permissive) is `Public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidentiality {
    /// Most restrictive (bottom).
    Secret,
    Restricted,
    /// Least restrictive (top).
    Public,
}

impl Confidentiality {
    fn rank(self) -> u8 {
        match self {
            Confidentiality::Secret => 0,
            Confidentiality::Restricted => 1,
            Confidentiality::Public => 2,
        }
    }
    pub fn to_str(self) -> &'static str {
        match self {
            Confidentiality::Secret => "secret",
            Confidentiality::Restricted => "restricted",
            Confidentiality::Public => "public",
        }
    }
    pub fn parse(s: &str) -> Option<Confidentiality> {
        match s {
            "secret" => Some(Confidentiality::Secret),
            "restricted" => Some(Confidentiality::Restricted),
            "public" => Some(Confidentiality::Public),
            _ => None,
        }
    }
}

/// IFC integrity label. Top (most permissive) is `Untrusted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    /// Most restrictive (bottom).
    Trusted,
    Endorsed,
    /// Least restrictive (top).
    Untrusted,
}

impl Integrity {
    fn rank(self) -> u8 {
        match self {
            Integrity::Trusted => 0,
            Integrity::Endorsed => 1,
            Integrity::Untrusted => 2,
        }
    }
    pub fn to_str(self) -> &'static str {
        match self {
            Integrity::Trusted => "trusted",
            Integrity::Endorsed => "endorsed",
            Integrity::Untrusted => "untrusted",
        }
    }
    pub fn parse(s: &str) -> Option<Integrity> {
        match s {
            "trusted" => Some(Integrity::Trusted),
            "endorsed" => Some(Integrity::Endorsed),
            "untrusted" => Some(Integrity::Untrusted),
            _ => None,
        }
    }
}

/// Linearity (usage count). Top (unbounded use) is `Omega`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linearity {
    Finite(u64),
    /// Unbounded use — the top.
    Omega,
}

impl Linearity {
    /// `self ⊑ other`: a finite bound is smaller than a larger finite bound and
    /// smaller than `omega`.
    pub fn le(self, other: Linearity) -> bool {
        match (self, other) {
            (_, Linearity::Omega) => true,
            (Linearity::Omega, Linearity::Finite(_)) => false,
            (Linearity::Finite(a), Linearity::Finite(b)) => a <= b,
        }
    }
    pub fn to_str(self) -> String {
        match self {
            Linearity::Omega => "omega".to_string(),
            Linearity::Finite(n) => n.to_string(),
        }
    }
    pub fn parse(s: &str) -> Option<Linearity> {
        match s {
            "omega" => Some(Linearity::Omega),
            other => other.parse::<u64>().ok().map(Linearity::Finite),
        }
    }
}

/// A point in the coefficient semiring `R`: one per surface in a v1 image.
#[derive(Debug, Clone, PartialEq)]
pub struct Coefficient {
    /// The timing multi-interval (`Interval^n`): one entry per delay path.
    pub timing: Vec<Interval>,
    pub confidentiality: Confidentiality,
    pub integrity: Integrity,
    pub linearity: Linearity,
}

impl Coefficient {
    /// The semiring **top** `⊤`: the maximally permissive coefficient, which is
    /// exactly what v0 has implicitly. `γ` embeds every v0 surface here.
    ///
    /// `top` uses a single, unbounded delay path — the canonical timing top.
    pub fn top() -> Coefficient {
        Coefficient {
            timing: vec![Interval::unbounded()],
            confidentiality: Confidentiality::Public,
            integrity: Integrity::Untrusted,
            linearity: Linearity::Omega,
        }
    }

    /// Whether this coefficient is the semiring top.
    pub fn is_top(&self) -> bool {
        self.timing.len() == 1
            && self.timing[0] == Interval::unbounded()
            && self.confidentiality == Confidentiality::Public
            && self.integrity == Integrity::Untrusted
            && self.linearity == Linearity::Omega
    }

    /// `self ⊑ other` in `R`: the pointwise product order across all four
    /// dimensions. Timing compares only when the multi-intervals have equal
    /// arity (delay-path count); differing arities are incomparable.
    pub fn leq(&self, other: &Coefficient) -> bool {
        let timing_le = self.timing.len() == other.timing.len()
            && self
                .timing
                .iter()
                .zip(&other.timing)
                .all(|(a, b)| a.contained_in(b));
        timing_le
            && self.confidentiality.rank() <= other.confidentiality.rank()
            && self.integrity.rank() <= other.integrity.rank()
            && self.linearity.le(other.linearity)
    }
}

/// Deterministic, round-trippable `f64` formatting for canonical output. Rust's
/// default `{}` already yields the shortest representation that round-trips.
fn fmt_f64(v: f64) -> String {
    if v == 0.0 {
        "0".to_string() // normalise -0.0
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_is_the_maximum() {
        let top = Coefficient::top();
        assert!(top.is_top());
        // A more-constrained coefficient is strictly below top.
        let constrained = Coefficient {
            timing: vec![Interval { lo: Bound::Finite(0.0), hi: Bound::Finite(16_000_000.0) }],
            confidentiality: Confidentiality::Secret,
            integrity: Integrity::Trusted,
            linearity: Linearity::Finite(1),
        };
        assert!(constrained.leq(&top));
        assert!(!top.leq(&constrained));
        assert!(!constrained.is_top());
    }

    #[test]
    fn interval_containment() {
        let wide = Interval { lo: Bound::Finite(-10.0), hi: Bound::Finite(10.0) };
        let narrow = Interval { lo: Bound::Finite(-1.0), hi: Bound::Finite(1.0) };
        assert!(narrow.contained_in(&wide));
        assert!(!wide.contained_in(&narrow));
        assert!(wide.contained_in(&Interval::unbounded()));
    }

    #[test]
    fn bound_roundtrips() {
        for b in [Bound::NegInf, Bound::PosInf, Bound::Finite(1.5), Bound::Finite(0.0)] {
            assert_eq!(Bound::parse(&b.to_str()), Some(b));
        }
        // -0.0 normalises to "0" on output, parsing back to +0.0.
        assert_eq!(Bound::parse(&Bound::Finite(-0.0).to_str()), Some(Bound::Finite(0.0)));
    }

    #[test]
    fn label_roundtrips() {
        for c in [Confidentiality::Secret, Confidentiality::Restricted, Confidentiality::Public] {
            assert_eq!(Confidentiality::parse(c.to_str()), Some(c));
        }
        for i in [Integrity::Trusted, Integrity::Endorsed, Integrity::Untrusted] {
            assert_eq!(Integrity::parse(i.to_str()), Some(i));
        }
        for l in [Linearity::Omega, Linearity::Finite(0), Linearity::Finite(7)] {
            assert_eq!(Linearity::parse(&l.to_str()), Some(l));
        }
    }

    #[test]
    fn differing_timing_arity_is_incomparable() {
        let one = Coefficient::top();
        let two = Coefficient {
            timing: vec![Interval::unbounded(), Interval::unbounded()],
            ..Coefficient::top()
        };
        assert!(!one.leq(&two));
        assert!(!two.leq(&one));
    }
}
