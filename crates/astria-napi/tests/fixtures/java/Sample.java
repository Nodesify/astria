import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * An LRU (Least Recently Used) cache implementation.
 *
 * <p>This cache evicts the least recently accessed entry when the
 * maximum capacity is reached. It supports O(1) get and put operations
 * backed by a hash map and a doubly-linked list.</p>
 */
public class Sample {

    /** Maximum number of entries the cache can hold. */
    private final int capacity;

    /** Map from key to its node in the linked list. */
    private final Map<String, Node> cache;

    /** Sentinel head of the doubly-linked list (most recently used). */
    private final Node head;

    /** Sentinel tail of the doubly-linked list (least recently used). */
    private final Node tail;

    /**
     * Internal node for the doubly-linked list.
     */
    private static class Node {
        String key;
        String value;
        Node prev;
        Node next;

        Node(String key, String value) {
            this.key = key;
            this.value = value;
        }
    }

    /**
     * Create a new LRU cache with the given capacity.
     *
     * @param capacity maximum number of entries; must be positive
     * @throws IllegalArgumentException if capacity is less than 1
     */
    public Sample(int capacity) {
        if (capacity < 1) {
            throw new IllegalArgumentException("Capacity must be at least 1");
        }
        this.capacity = capacity;
        this.cache = new HashMap<>();
        this.head = new Node(null, null);
        this.tail = new Node(null, null);
        head.next = tail;
        tail.prev = head;
    }

    /**
     * Retrieve a value from the cache.
     *
     * <p>NOTE: Accessing a key promotes it to most-recently-used, which
     * is the defining behavior of an LRU cache.</p>
     *
     * @param key the key to look up
     * @return the cached value, or null if not present
     */
    public String get(String key) {
        Node node = cache.get(key);
        if (node == null) {
            return null;
        }
        removeNode(node);
        addToFront(node);
        return node.value;
    }

    /**
     * Insert or update a key-value pair.
     *
     * <p>WHY: We eagerly evict when over capacity instead of checking
     * on the next insert. This keeps the size invariant simple and
     * prevents subtle bugs in multi-threaded scenarios.</p>
     *
     * @param key   the key to insert
     * @param value the value to associate
     */
    public void put(String key, String value) {
        Node existing = cache.get(key);
        if (existing != null) {
            existing.value = value;
            removeNode(existing);
            addToFront(existing);
            return;
        }
        Node node = new Node(key, value);
        cache.put(key, node);
        addToFront(node);
        if (cache.size() > capacity) {
            Node lru = tail.prev;
            removeNode(lru);
            cache.remove(lru.key);
        }
    }

    /** Remove a node from its current position in the linked list. */
    private void removeNode(Node node) {
        node.prev.next = node.next;
        node.next.prev = node.prev;
    }

    /** Insert a node right after the head sentinel. */
    private void addToFront(Node node) {
        node.next = head.next;
        node.prev = head;
        head.next.prev = node;
        head.next = node;
    }

    /**
     * Return an unmodifiable snapshot of all keys in MRU order.
     *
     * @return list of keys from most to least recently used
     */
    public List<String> keys() {
        List<String> result = new ArrayList<>();
        Node current = head.next;
        while (current != tail) {
            result.add(current.key);
            current = current.next;
        }
        return Collections.unmodifiableList(result);
    }

    /**
     * Return the number of entries currently in the cache.
     */
    public int size() {
        return cache.size();
    }
}
