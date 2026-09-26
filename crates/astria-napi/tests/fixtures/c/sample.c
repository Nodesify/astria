/*
 * Dynamic array implementation for generic pointer-based storage.
 *
 * Provides a growable vector that doubles in capacity when full,
 * similar to a simplified C++ std::vector<void*>.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/** Dynamic array holding generic pointers. */
typedef struct {
    void   **items;     /* heap-allocated element array */
    size_t  count;      /* number of active elements */
    size_t  capacity;   /* allocated slots in items */
} DynArray;

/*
 * WHY: Storing function pointers as the destructor callback lets us
 * avoid a hard dependency on a specific free() strategy — callers
 * can pass NULL for POD arrays or a custom cleanup for nested structs.
 */
typedef void (*DestroyFn)(void *);

/** Create a new DynArray with the given initial capacity. */
DynArray dynarray_create(size_t initial_capacity) {
    DynArray arr;
    arr.capacity = initial_capacity < 4 ? 4 : initial_capacity;
    arr.count = 0;
    arr.items = malloc(sizeof(void *) * arr.capacity);
    if (!arr.items) {
        fprintf(stderr, "dynarray_create: out of memory\n");
        exit(1);
    }
    return arr;
}

/** Double the underlying storage if the array is full. */
static void dynarray_grow(DynArray *arr) {
    /* NOTE: We use geometric growth (2x) so that amortized append
       cost stays O(1) instead of O(n) with linear growth. */
    size_t new_capacity = arr->capacity * 2;
    void **new_items = realloc(arr->items, sizeof(void *) * new_capacity);
    if (!new_items) {
        fprintf(stderr, "dynarray_grow: out of memory\n");
        exit(1);
    }
    arr->items = new_items;
    arr->capacity = new_capacity;
}

/** Append an item to the end of the array, growing if needed. */
void dynarray_push(DynArray *arr, void *item) {
    if (arr->count == arr->capacity) {
        dynarray_grow(arr);
    }
    arr->items[arr->count++] = item;
}

/** Remove the last item and return it. Returns NULL if empty. */
void *dynarray_pop(DynArray *arr) {
    if (arr->count == 0) {
        return NULL;
    }
    return arr->items[--arr->count];
}

/** Return the item at index, or NULL if out of bounds. */
void *dynarray_get(DynArray *arr, size_t index) {
    if (index >= arr->count) {
        return NULL;
    }
    return arr->items[index];
}

/** Free the array and optionally each element via destroy. */
void dynarray_destroy(DynArray *arr, DestroyFn destroy) {
    if (destroy) {
        for (size_t i = 0; i < arr->count; i++) {
            destroy(arr->items[i]);
        }
    }
    free(arr->items);
    arr->items = NULL;
    arr->count = 0;
    arr->capacity = 0;
}

/* --- Example usage --------------------------------------------------- */

typedef struct {
    int    id;
    char   name[64];
} Record;

static void record_destroy(void *ptr) {
    free(ptr);
}

int main(void) {
    DynArray arr = dynarray_create(4);

    for (int i = 0; i < 10; i++) {
        Record *r = malloc(sizeof(Record));
        r->id = i;
        snprintf(r->name, sizeof(r->name), "record_%d", i);
        dynarray_push(&arr, r);
    }

    printf("Array contains %zu items (capacity %zu)\n", arr.count, arr.capacity);

    /* HACK: Casting away void* to Record* without a type tag works here
       because we know all entries are Record pointers. A production
       library would embed a type discriminator. */
    Record *first = (Record *) dynarray_get(&arr, 0);
    printf("First: id=%d name=%s\n", first->id, first->name);

    dynarray_destroy(&arr, record_destroy);
    return 0;
}
