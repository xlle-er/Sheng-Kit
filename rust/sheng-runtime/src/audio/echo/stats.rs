//! Echo diagnostics for the snapshot `echo` field. Only counters and energies; no audio.

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EchoStats {
    pub frames: u64,
    pub matched_references: u64,
    pub missing_references: u64,
    pub resets: u64,
    input_energy: f64,
    output_energy: f64,
    reference_energy: f64,
    samples: u64,
    reference_samples: u64,
    /// Energies over matched frames only, for the reduction figure.
    matched_input_energy: f64,
    matched_output_energy: f64,
}

fn energy(samples: &[i16]) -> f64 {
    samples
        .iter()
        .map(|s| {
            let x = f64::from(*s) / 32768.0;
            x * x
        })
        .sum()
}

fn rms(energy: f64, count: u64) -> Option<f64> {
    (count > 0).then(|| (energy / count as f64).sqrt())
}

impl EchoStats {
    pub fn record(&mut self, input: &[i16], output: &[i16], reference: Option<&[i16]>) {
        let (input_energy, output_energy) = (energy(input), energy(output));
        self.input_energy += input_energy;
        self.output_energy += output_energy;
        self.samples += input.len() as u64;
        if let Some(reference) = reference {
            self.reference_energy += energy(reference);
            self.reference_samples += reference.len() as u64;
            self.matched_input_energy += input_energy;
            self.matched_output_energy += output_energy;
        }
    }

    /// Snapshot form (snapshot.md `echo`).
    pub fn report(&self) -> EchoReport {
        let reduction = (self.matched_output_energy > 0.0 && self.matched_input_energy > 0.0)
            .then(|| 10.0 * (self.matched_input_energy / self.matched_output_energy).log10());
        EchoReport {
            frames: self.frames,
            matched_references: self.matched_references,
            missing_references: self.missing_references,
            resets: self.resets,
            input_rms: rms(self.input_energy, self.samples),
            output_rms: rms(self.output_energy, self.samples),
            reference_rms: rms(self.reference_energy, self.reference_samples),
            reduction_db: reduction,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EchoReport {
    pub frames: u64,
    pub matched_references: u64,
    pub missing_references: u64,
    pub resets: u64,
    pub input_rms: Option<f64>,
    pub output_rms: Option<f64>,
    pub reference_rms: Option<f64>,
    pub reduction_db: Option<f64>,
}
