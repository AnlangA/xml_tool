/// Caching system for XML Tool
///
/// Provides LRU caching for expensive operations like search results,
/// serialization, and tree flattening.
use lru::LruCache;
use parking_lot::RwLock;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;

/// Generic LRU cache with thread-safe access
pub struct Cache<K, V>
where
    K: Hash + Eq,
{
    cache: Arc<RwLock<LruCache<K, V>>>,
}

impl<K, V> Clone for Cache<K, V>
where
    K: Hash + Eq,
{
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
        }
    }
}

impl<K, V> Cache<K, V>
where
    K: Hash + Eq,
{
    /// Create a new cache with the given capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
        }
    }

    /// Get a value from the cache
    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.cache.write().get(key).cloned()
    }

    /// Insert a value into the cache
    pub fn insert(&self, key: K, value: V) {
        self.cache.write().put(key, value);
    }

    /// Check if the cache contains a key
    pub fn contains(&self, key: &K) -> bool {
        self.cache.read().contains(key)
    }

    /// Clear the cache
    pub fn clear(&self) {
        self.cache.write().clear();
    }

    /// Get the number of items in the cache
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    /// Get or insert a value using a closure
    pub fn get_or_insert_with<F>(&self, key: K, f: F) -> V
    where
        K: Clone,
        V: Clone,
        F: FnOnce() -> V,
    {
        // Try to get from cache first
        if let Some(value) = self.get(&key) {
            return value;
        }

        // Compute and insert
        let value = f();
        self.insert(key, value.clone());
        value
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchResults {
    pub visible_elements: Vec<u64>,
    pub matched_elements: Vec<u64>,
    pub name_matches: Vec<u64>,
}

impl SearchResults {
    pub fn contains_visible_element(&self, id: u64) -> bool {
        self.visible_elements.binary_search(&id).is_ok()
    }

    pub fn contains_name_match(&self, id: u64) -> bool {
        self.name_matches.binary_search(&id).is_ok()
    }
}

/// Search result cache
pub type SearchCache = Cache<SearchKey, Arc<SearchResults>>;

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct SearchKey {
    pub query: String,
    pub case_sensitive: bool,
    pub doc_version: u64,
}

impl SearchKey {
    pub fn new(query: String, case_sensitive: bool, doc_version: u64) -> Self {
        Self {
            query,
            case_sensitive,
            doc_version,
        }
    }
}

/// Serialization cache
pub type SerializationCache = Cache<u64, String>;

/// Tree flattening cache
pub type TreeCache = Cache<TreeCacheKey, Vec<u64>>;

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct TreeCacheKey {
    pub doc_version: u64,
    pub expansion_state: Vec<(u64, bool)>, // (node_id, is_expanded)
}

/// Cache manager that holds all caches
pub struct CacheManager {
    pub search: SearchCache,
    pub serialization: SerializationCache,
    pub tree: TreeCache,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            search: Cache::new(50),        // Cache 50 search results
            serialization: Cache::new(10), // Cache 10 serializations
            tree: Cache::new(20),          // Cache 20 tree states
        }
    }

    /// Clear all caches
    pub fn clear_all(&self) {
        self.search.clear();
        self.serialization.clear();
        self.tree.clear();
    }

    /// Invalidate caches for a specific document version
    pub fn invalidate_document(&self, _version: u64) {
        // For now, just clear all caches
        // In the future, we could be more selective
        self.clear_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic_operations() {
        let cache: Cache<String, i32> = Cache::new(3);

        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);

        assert_eq!(cache.get(&"a".to_string()), Some(1));
        assert_eq!(cache.get(&"b".to_string()), Some(2));
        assert_eq!(cache.get(&"c".to_string()), None);

        assert_eq!(cache.len(), 2);
        assert!(!cache.is_empty());
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache: Cache<String, i32> = Cache::new(2);

        cache.insert("a".to_string(), 1);
        cache.insert("b".to_string(), 2);
        cache.insert("c".to_string(), 3); // Should evict "a"

        assert_eq!(cache.get(&"a".to_string()), None);
        assert_eq!(cache.get(&"b".to_string()), Some(2));
        assert_eq!(cache.get(&"c".to_string()), Some(3));
    }

    #[test]
    fn test_cache_get_or_insert() {
        let cache: Cache<String, i32> = Cache::new(10);

        let mut call_count = 0;

        let value1 = cache.get_or_insert_with("key".to_string(), || {
            call_count += 1;
            42
        });

        assert_eq!(value1, 42);
        assert_eq!(call_count, 1);

        // Second call should use cached value
        let value2 = cache.get_or_insert_with("key".to_string(), || {
            call_count += 1;
            99
        });

        assert_eq!(value2, 42); // Still 42, not 99
        assert_eq!(call_count, 1); // Closure not called again
    }

    #[test]
    fn test_search_cache_key() {
        let key1 = SearchKey::new("test".to_string(), true, 1);
        let key2 = SearchKey::new("test".to_string(), true, 1);
        let key3 = SearchKey::new("test".to_string(), false, 1);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_cache_manager() {
        let manager = CacheManager::new();

        manager.search.insert(
            SearchKey::new("query".to_string(), true, 1),
            Arc::new(SearchResults {
                visible_elements: vec![1, 2, 3],
                matched_elements: vec![2],
                name_matches: vec![2],
            }),
        );

        assert_eq!(manager.search.len(), 1);

        manager.clear_all();

        assert_eq!(manager.search.len(), 0);
    }
}
