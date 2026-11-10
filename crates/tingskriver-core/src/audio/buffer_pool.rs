use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

/// Audio buffer pool for reducing memory allocations during recording
pub struct AudioBufferPool {
    pool: Arc<Mutex<VecDeque<Vec<f32>>>>,
    max_size: usize,
    buffer_capacity: usize,
}

impl AudioBufferPool {
    /// Create a new audio buffer pool with specified maximum pool size and buffer capacity
    pub fn new(max_size: usize, buffer_capacity: usize) -> Self {
        Self {
            pool: Arc::new(Mutex::new(VecDeque::with_capacity(max_size))),
            max_size,
            buffer_capacity,
        }
    }

    /// Get a buffer from the pool, or create a new one if pool is empty
    pub fn get_buffer(&self) -> Vec<f32> {
        let mut pool = self
            .pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match pool.pop_front() {
            Some(mut buffer) => {
                buffer.clear();
                buffer.reserve(self.buffer_capacity);
                buffer
            }
            None => {
                // Pool is empty, create a new buffer
                Vec::with_capacity(self.buffer_capacity)
            }
        }
    }

    /// Return a buffer to the pool for reuse
    pub fn return_buffer(&self, mut buffer: Vec<f32>) {
        // Clear the buffer but keep its allocated capacity
        buffer.clear();

        let mut pool = self
            .pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Only keep buffers if we haven't exceeded max pool size
        if pool.len() < self.max_size {
            pool.push_back(buffer);
        }
        // If pool is full, let the buffer be dropped (deallocated)
    }

    /// Get current pool size (for monitoring)
    pub fn pool_size(&self) -> usize {
        self.pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Clear all buffers in the pool
    pub fn clear(&self) {
        self.pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

impl Clone for AudioBufferPool {
    fn clone(&self) -> Self {
        Self {
            pool: Arc::clone(&self.pool),
            max_size: self.max_size,
            buffer_capacity: self.buffer_capacity,
        }
    }
}

/// RAII wrapper that automatically returns buffer to pool when dropped
pub struct PooledBuffer {
    buffer: Vec<f32>,
    pool: AudioBufferPool,
    return_to_pool: bool,
}

impl PooledBuffer {
    /// Create a new pooled buffer
    pub fn new(pool: AudioBufferPool) -> Self {
        let buffer = pool.get_buffer();
        Self {
            buffer,
            pool,
            return_to_pool: true,
        }
    }

    /// Get mutable access to the underlying buffer
    pub fn as_mut(&mut self) -> &mut Vec<f32> {
        &mut self.buffer
    }

    /// Get immutable access to the underlying buffer
    pub fn as_ref(&self) -> &Vec<f32> {
        &self.buffer
    }

    /// Consume the wrapper and return the buffer (will not be returned to pool)
    pub fn into_inner(mut self) -> Vec<f32> {
        self.return_to_pool = false;
        std::mem::take(&mut self.buffer)
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if self.return_to_pool {
            self.pool.return_buffer(std::mem::take(&mut self.buffer));
        }
    }
}

impl std::ops::Deref for PooledBuffer {
    type Target = Vec<f32>;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl std::ops::DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_remains_usable_after_a_worker_panics() {
        let pool = AudioBufferPool::new(2, 32);
        let worker_pool = pool.clone();
        assert!(std::thread::spawn(move || {
            let _guard = worker_pool.pool.lock();
            panic!("simulate a worker failure while holding the pool");
        })
        .join()
        .is_err());
        let mut buffer = pool.get_buffer();
        buffer.push(1.0);
        pool.return_buffer(buffer);
        assert_eq!(pool.pool_size(), 1);
        assert!(pool.get_buffer().is_empty());
        pool.clear();
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn into_inner_keeps_samples_and_does_not_return_the_buffer() {
        let pool = AudioBufferPool::new(2, 32);
        let mut buffer = PooledBuffer::new(pool.clone());
        buffer.extend_from_slice(&[0.25, -0.5]);
        assert_eq!(buffer.into_inner(), vec![0.25, -0.5]);
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn empty_buffers_are_returned_unless_consumed() {
        let pool = AudioBufferPool::new(2, 0);
        drop(PooledBuffer::new(pool.clone()));
        assert_eq!(pool.pool_size(), 1);
        assert!(PooledBuffer::new(pool.clone()).into_inner().is_empty());
        assert_eq!(pool.pool_size(), 0);
    }

    #[test]
    fn test_buffer_pool() {
        let pool = AudioBufferPool::new(3, 1024);
        assert_eq!(pool.pool_size(), 0);

        // Get a buffer and return it
        let buffer = pool.get_buffer();
        assert_eq!(buffer.capacity(), 1024);
        pool.return_buffer(buffer);
        assert_eq!(pool.pool_size(), 1);

        // Get it back
        let buffer2 = pool.get_buffer();
        assert_eq!(pool.pool_size(), 0);
        pool.return_buffer(buffer2);
    }

    #[test]
    fn test_pooled_buffer_raii() {
        let pool = AudioBufferPool::new(2, 512);

        {
            let mut pooled = PooledBuffer::new(pool.clone());
            pooled.push(1.0);
            pooled.push(2.0);
            assert_eq!(pooled.len(), 2);
        } // Buffer should be returned to pool here

        assert_eq!(pool.pool_size(), 1);
    }

    #[test]
    fn test_pool_max_size() {
        let pool = AudioBufferPool::new(2, 256);

        // Fill the pool to capacity
        let buf1 = pool.get_buffer();
        let buf2 = pool.get_buffer();
        let buf3 = pool.get_buffer();

        pool.return_buffer(buf1);
        pool.return_buffer(buf2);
        pool.return_buffer(buf3); // This one should be dropped since pool is full

        assert_eq!(pool.pool_size(), 2);
    }
}
