use crate::metrics::{DriveInfo, Metric, Metrics};

const CAPACITY: usize = 120;

pub struct History {
    pub cpu: Vec<f32>,
    pub ram: Vec<f32>,
    pub disk: Vec<f32>,
    pub cap: Vec<f32>,
    pub net: Vec<f32>,
}

impl History {
    pub fn new() -> History {
        History {
            cpu: Vec::with_capacity(CAPACITY),
            ram: Vec::with_capacity(CAPACITY),
            disk: Vec::with_capacity(CAPACITY),
            cap: Vec::with_capacity(CAPACITY),
            net: Vec::with_capacity(CAPACITY),
        }
    }

    pub fn push(&mut self, m: &Metrics, drives: &[DriveInfo]) {
        push(&mut self.cpu, m.cpu.clamp(0.0, 100.0));
        push(&mut self.ram, m.mem_pct as f32);
        push(&mut self.disk, m.disk_busy.clamp(0.0, 100.0));
        let top = drives
            .iter()
            .map(|d| d.used_pct())
            .fold(0.0f32, |a, b| a.max(b));
        push(&mut self.cap, top);
        push(&mut self.net, (m.rx + m.tx) as f32);
    }

    pub fn series(&self, m: Metric) -> &[f32] {
        match m {
            Metric::Cpu => &self.cpu,
            Metric::Ram => &self.ram,
            Metric::Disk => &self.disk,
            Metric::Capacity => &self.cap,
            Metric::Net => &self.net,
        }
    }
}

fn push(v: &mut Vec<f32>, x: f32) {
    if v.len() == CAPACITY {
        v.remove(0);
    }
    v.push(x);
}
