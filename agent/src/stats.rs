//! Gecikme istatistikleri (son N örnek).
use serde_json::{json, Value};
use std::collections::VecDeque;

const KEEP: usize = 500;

#[derive(Default)]
pub struct Stats {
    pub one_way: VecDeque<f64>,
    pub proc: VecDeque<f64>,
    pub rate: u32,
}

fn push(q: &mut VecDeque<f64>, v: f64) {
    if q.len() == KEEP {
        q.pop_front();
    }
    q.push_back(v);
}

impl Stats {
    pub fn add_one_way(&mut self, v: f64) {
        push(&mut self.one_way, v);
    }
    pub fn add_proc(&mut self, v: f64) {
        push(&mut self.proc, v);
    }
    pub fn summary(&self) -> Value {
        json!({ "oneWay": percentiles(&self.one_way), "proc": percentiles(&self.proc), "rate": self.rate })
    }
}

pub fn percentiles(q: &VecDeque<f64>) -> Value {
    if q.is_empty() {
        return Value::Null;
    }
    let mut s: Vec<f64> = q.iter().copied().collect();
    s.sort_by(|a, b| a.total_cmp(b));
    let p = |f: f64| {
        let v = s[((f * s.len() as f64) as usize).min(s.len() - 1)];
        (v * 100.0).round() / 100.0
    };
    json!({ "n": s.len(), "p50": p(0.5), "p95": p(0.95), "p99": p(0.99), "max": p(1.0) })
}
