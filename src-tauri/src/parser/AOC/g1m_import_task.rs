use super::{G1mTextureResolution, PARALLEL_IMPORT_MIN_BYTES};
use std::{io, thread};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static TEST_WORKER_LIMIT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(super) fn set_test_worker_limit(limit: usize) {
    TEST_WORKER_LIMIT.store(limit, Ordering::Relaxed);
}

/// Coordinates the bounded worker set used by large G1M imports.
pub struct G1mImportTask {
    data_len: usize,
    section_count: usize,
}

impl G1mImportTask {
    pub fn new(data_len: usize, section_count: usize) -> Self {
        Self {
            data_len,
            section_count,
        }
    }

    pub fn worker_count(&self) -> usize {
        #[cfg(test)]
        let test_limit = TEST_WORKER_LIMIT.load(Ordering::Relaxed);
        #[cfg(not(test))]
        let test_limit = 0;
        if self.data_len < PARALLEL_IMPORT_MIN_BYTES {
            1
        } else {
            self.section_count
                .clamp(1, 4)
                .min(if test_limit == 0 { 4 } else { test_limit })
        }
    }

    pub fn run<'scope, T, M, X>(
        &self,
        mesh_task: M,
        texture_task: Option<X>,
    ) -> io::Result<(Vec<T>, Option<G1mTextureResolution>)>
    where
        T: Send + 'scope,
        M: Fn(usize, usize) -> io::Result<Vec<T>> + Sync + 'scope,
        X: FnOnce() -> G1mTextureResolution + Send + 'scope,
    {
        if self.data_len < PARALLEL_IMPORT_MIN_BYTES {
            return Ok((mesh_task(0, 1)?, texture_task.map(|task| task())));
        }
        let workers = self.worker_count();
        if workers == 1 {
            return Ok((mesh_task(0, 1)?, texture_task.map(|task| task())));
        }
        thread::scope(|scope| {
            let texture = texture_task.map(|task| scope.spawn(task));
            let mesh_task = &mesh_task;
            let handles: Vec<_> = (0..workers)
                .map(|worker| scope.spawn(move || mesh_task(worker, workers)))
                .collect();
            let mut output = Vec::new();
            for handle in handles {
                output.extend(
                    handle
                        .join()
                        .map_err(|_| io::Error::other("G1M 3D worker panicked"))??,
                );
            }
            let textures = texture
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| io::Error::other("G1M texture worker panicked"))
                })
                .transpose()?;
            Ok((output, textures))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_imports_use_at_most_four_3d_workers() {
        assert_eq!(
            G1mImportTask::new(PARALLEL_IMPORT_MIN_BYTES, 12).worker_count(),
            4
        );
        assert_eq!(
            G1mImportTask::new(PARALLEL_IMPORT_MIN_BYTES, 3).worker_count(),
            3
        );
    }

    #[test]
    fn small_imports_remain_single_threaded() {
        assert_eq!(
            G1mImportTask::new(PARALLEL_IMPORT_MIN_BYTES - 1, 12).worker_count(),
            1
        );
    }
}
