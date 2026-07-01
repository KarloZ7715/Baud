//! Cache de mascaras builtin por (char, cell_w, cell_h).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    ch: char,
    w: u32,
    h: u32,
}

/// Cache thread-safe de mascaras alpha (compartida por el renderer).
#[derive(Debug, Default)]
pub struct MaskCache {
    inner: Mutex<HashMap<CacheKey, Arc<[u8]>>>,
}

impl MaskCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_insert<F>(&self, ch: char, w: u32, h: u32, build: &mut F) -> Option<Arc<[u8]>>
    where
        F: FnMut() -> Option<Vec<u8>>,
    {
        if w == 0 || h == 0 {
            return None;
        }
        let key = CacheKey { ch, w, h };
        let mut guard = self.inner.lock().ok()?;
        if let Some(hit) = guard.get(&key) {
            return Some(Arc::clone(hit));
        }
        let data = build()?;
        let arc: Arc<[u8]> = Arc::from(data.into_boxed_slice());
        guard.insert(key, Arc::clone(&arc));
        Some(arc)
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.clear();
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_on_second_lookup() {
        let cache = MaskCache::new();
        let mut built = 0u32;
        let mut build = || {
            built += 1;
            Some(vec![255u8; 10 * 20])
        };
        assert!(cache.get_or_insert('x', 10, 20, &mut build).is_some());
        assert!(cache.get_or_insert('x', 10, 20, &mut build).is_some());
        assert_eq!(built, 1);
        assert_eq!(cache.len(), 1);
    }
}
