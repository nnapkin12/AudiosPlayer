use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Bars drawn by the now-playing canvas.
pub const BANDS: usize = 32;
const FFT_LEN: usize = 256;
const RING: usize = 512;

/// Decimated mono frames from the decode thread. The audio callback only
/// stores a sample. The spectrum is computed off that thread.
pub struct VizTap {
    enabled: AtomicBool,
    write: AtomicUsize,
    rate: AtomicU32,
    samples: Box<[AtomicU32]>,
}

impl Default for VizTap {
    fn default() -> Self {
        Self::new()
    }
}

impl VizTap {
    pub fn new() -> Self {
        let samples = (0..RING).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Self {
            enabled: AtomicBool::new(false),
            write: AtomicUsize::new(0),
            rate: AtomicU32::new(48_000),
            samples: samples.into_boxed_slice(),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.write.store(0, Ordering::Relaxed);
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn push(&self, sample: f32, rate: u32) {
        let index = self.write.load(Ordering::Relaxed);
        self.samples[index % RING].store(sample.to_bits(), Ordering::Relaxed);
        let current = self.rate.load(Ordering::Relaxed);
        if current != rate && rate > 0 {
            self.rate.store(rate, Ordering::Relaxed);
        }
        self.write.store(index.wrapping_add(1), Ordering::Release);
    }

    pub fn spectrum(&self) -> Option<[u8; BANDS]> {
        if !self.enabled() {
            return None;
        }
        let end = self.write.load(Ordering::Acquire);
        if end < FFT_LEN {
            return None;
        }
        let mut window = [0.0f32; FFT_LEN];
        for (offset, sample) in window.iter_mut().enumerate() {
            let index = end - FFT_LEN + offset;
            *sample = f32::from_bits(self.samples[index % RING].load(Ordering::Relaxed));
        }
        let rate = self.rate.load(Ordering::Relaxed).max(1);
        Some(bands_from(&window, rate))
    }
}

fn bands_from(window: &[f32; FFT_LEN], _rate: u32) -> [u8; BANDS] {
    let mag = fft_magnitude(window);
    let mut out = [0u8; BANDS];
    let last = (FFT_LEN / 2) as f32;
    for band in 0..BANDS {
        let start = log_bin(band as f32 / BANDS as f32, last);
        let end = log_bin((band as f32 + 1.0) / BANDS as f32, last).max(start + 1);
        let mut energy = 0.0f32;
        let mut count = 0u32;
        for bin in start..end.min(FFT_LEN / 2) {
            energy += mag[bin] * mag[bin];
            count += 1;
        }
        let level = if count == 0 {
            0.0
        } else {
            (energy / count as f32).sqrt()
        };
        let shaped = (level * 4.0).clamp(0.0, 1.0).powf(0.65);
        out[band] = (shaped * 255.0).round() as u8;
    }
    out
}

fn log_bin(t: f32, last: f32) -> usize {
    last.powf(t).floor() as usize
}

fn fft_magnitude(input: &[f32; FFT_LEN]) -> [f32; FFT_LEN / 2] {
    let mut re = [0.0f32; FFT_LEN];
    let mut im = [0.0f32; FFT_LEN];
    let window = hann();
    for (index, sample) in input.iter().enumerate() {
        re[index] = sample * window[index];
    }
    bit_reverse(&mut re, &mut im);
    let mut len = 2;
    while len <= FFT_LEN {
        let half = len / 2;
        let angle = -std::f32::consts::PI / half as f32;
        let step_re = angle.cos();
        let step_im = angle.sin();
        let mut start = 0;
        while start < FFT_LEN {
            let mut w_re = 1.0f32;
            let mut w_im = 0.0f32;
            for offset in 0..half {
                let even = start + offset;
                let odd = even + half;
                let t_re = w_re * re[odd] - w_im * im[odd];
                let t_im = w_re * im[odd] + w_im * re[odd];
                re[odd] = re[even] - t_re;
                im[odd] = im[even] - t_im;
                re[even] += t_re;
                im[even] += t_im;
                let next_re = w_re * step_re - w_im * step_im;
                w_im = w_re * step_im + w_im * step_re;
                w_re = next_re;
            }
            start += len;
        }
        len *= 2;
    }
    let scale = (FFT_LEN / 2) as f32;
    let mut mag = [0.0f32; FFT_LEN / 2];
    for index in 0..FFT_LEN / 2 {
        mag[index] = re[index].hypot(im[index]) / scale;
    }
    mag
}

fn bit_reverse(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j >= bit {
            j -= bit;
            bit >>= 1;
        }
        j += bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
}

fn hann() -> &'static [f32; FFT_LEN] {
    use std::sync::OnceLock;
    static HANN: OnceLock<[f32; FFT_LEN]> = OnceLock::new();
    HANN.get_or_init(|| {
        let mut window = [0.0f32; FFT_LEN];
        for (index, value) in window.iter_mut().enumerate() {
            let phase = 2.0 * std::f32::consts::PI * index as f32 / FFT_LEN as f32;
            *value = 0.5 * (1.0 - phase.cos());
        }
        window
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_flat() {
        let window = [0.0f32; FFT_LEN];
        assert_eq!(bands_from(&window, 12_000), [0; BANDS]);
    }

    #[test]
    fn sine_peaks_in_one_region() {
        let rate = 12_000u32;
        let freq = 1_000.0f32;
        let mut window = [0.0f32; FFT_LEN];
        for (index, sample) in window.iter_mut().enumerate() {
            let phase = 2.0 * std::f32::consts::PI * freq * index as f32 / rate as f32;
            *sample = phase.sin();
        }
        let bands = bands_from(&window, rate);
        let peak = bands
            .iter()
            .enumerate()
            .max_by_key(|(_, value)| *value)
            .map(|(index, _)| index)
            .unwrap();
        let bin = freq * FFT_LEN as f32 / rate as f32;
        let mut expected = 0usize;
        let last = (FFT_LEN / 2) as f32;
        for band in 0..BANDS {
            let start = log_bin(band as f32 / BANDS as f32, last);
            let end = log_bin((band as f32 + 1.0) / BANDS as f32, last).max(start + 1);
            if (start as f32) <= bin && bin < end as f32 {
                expected = band;
                break;
            }
        }
        assert!(
            peak.abs_diff(expected) <= 1,
            "peak band {peak}, expected {expected}, bands {bands:?}"
        );
        assert!(bands[peak] > 40, "peak too quiet: {bands:?}");
    }
}
