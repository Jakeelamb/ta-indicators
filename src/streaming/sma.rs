




use std::collections::VecDeque;




#[derive(Debug, Clone)]
pub struct IncrementalSma {
    period: usize,
    buffer: VecDeque<f64>,
    sum: f64,
}

impl IncrementalSma {

    pub fn new(period: usize) -> Self {
        Self {
            period,
            buffer: VecDeque::with_capacity(period),
            sum: 0.0,
        }
    }




    #[inline]
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.sum += value;

        if self.buffer.len() >= self.period {

            if let Some(oldest) = self.buffer.pop_front() {
                self.sum -= oldest;
            }
        }

        self.buffer.push_back(value);

        if self.buffer.len() >= self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }


    #[inline]
    pub fn current(&self) -> Option<f64> {
        if self.buffer.len() >= self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }




    pub fn compute_batch(values: &[f64], period: usize) -> Vec<f64> {
        let mut result = vec![f64::NAN; values.len()];
        let mut sma = Self::new(period);

        for (i, &v) in values.iter().enumerate() {
            if let Some(val) = sma.update(v) {
                result[i] = val;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sma_basic() {
        let mut sma = IncrementalSma::new(3);

        assert!(sma.update(1.0).is_none());
        assert!(sma.update(2.0).is_none());
        let result = sma.update(3.0);
        assert!((result.unwrap() - 2.0).abs() < 1e-10);

        let result = sma.update(4.0);
        assert!((result.unwrap() - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_sma_batch() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = IncrementalSma::compute_batch(&values, 3);

        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert!((result[2] - 2.0).abs() < 1e-10);
        assert!((result[3] - 3.0).abs() < 1e-10);
        assert!((result[4] - 4.0).abs() < 1e-10);
    }
}
