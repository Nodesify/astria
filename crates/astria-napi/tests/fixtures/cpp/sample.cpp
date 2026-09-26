/**
 * Thread-safe task queue with prioritized execution.
 *
 * Provides a templated priority queue that supports concurrent
 * producers and a single consumer (MPSC pattern).
 */

#include <condition_variable>
#include <functional>
#include <iostream>
#include <mutex>
#include <queue>
#include <string>
#include <thread>
#include <vector>

/**
 * Represents a unit of work with an associated priority.
 *
 * Lower priority values are dequeued first (min-heap ordering).
 */
class Task {
public:
    Task(int priority, std::string name, std::function<void()> handler)
        : priority_(priority), name_(std::move(name)), handler_(std::move(handler)) {}

    /** Execute the stored handler. */
    void run() const {
        // NOTE: We catch exceptions inside the handler so that a
        // failing task never crashes the consumer thread.
        try {
            handler_();
        } catch (const std::exception& e) {
            std::cerr << "Task '" << name_ << "' failed: " << e.what() << "\n";
        }
    }

    int priority() const { return priority_; }
    std::string name() const { return name_; }

    /** Comparator so std::priority_queue orders by ascending priority. */
    bool operator>(const Task& other) const {
        return priority_ > other.priority_;
    }

private:
    int priority_;
    std::string name_;
    std::function<void()> handler_;
};

/**
 * Multi-producer, single-consumer task queue.
 *
 * WHY: Using a condition variable instead of a spin-loop avoids burning
 * CPU cycles when the queue is empty — critical for long-running services.
 */
class TaskQueue {
public:
    /** Enqueue a task (thread-safe, callable from any producer). */
    void push(Task task) {
        {
            std::lock_guard<std::mutex> lock(mutex_);
            queue_.push(std::move(task));
        }
        cv_.notify_one();
    }

    /**
     * Dequeue and execute tasks until stopped.
     * Intended to run on a dedicated consumer thread.
     */
    void consume() {
        while (true) {
            Task task = waitAndPop();
            if (stopped_) break;
            task.run();
        }
    }

    /** Signal the consumer thread to stop after draining the queue. */
    void stop() {
        {
            std::lock_guard<std::mutex> lock(mutex_);
            stopped_ = true;
        }
        cv_.notify_one();
    }

    /** Return the current number of pending tasks. */
    size_t size() const {
        std::lock_guard<std::mutex> lock(mutex_);
        return queue_.size();
    }

private:
    /** Block until a task is available or stop is signaled. */
    Task waitAndPop() {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_.wait(lock, [this] { return !queue_.empty() || stopped_; });
        if (stopped_ && queue_.empty()) {
            return Task(0, "", [] {});
        }
        Task task = std::move(const_cast<Task&>(queue_.top()));
        queue_.pop();
        return task;
    }

    mutable std::mutex mutex_;
    std::condition_variable cv_;
    std::priority_queue<Task, std::vector<Task>, std::greater<Task>> queue_;
    bool stopped_ = false;
};

/**
 * Helper: submit a batch of trivial tasks to the queue.
 * Each task simply prints its name.
 */
void submitBatch(TaskQueue& queue, int count) {
    for (int i = 0; i < count; i++) {
        std::string name = "task_" + std::to_string(i);
        // HACK: Capturing `name` by value in the lambda is fine here,
        // but beware of capturing references to loop variables.
        queue.push(Task(i % 10, name, [name]() {
            std::cout << "Completed " << name << "\n";
        }));
    }
}

int main() {
    TaskQueue queue;

    // Start consumer thread
    std::thread consumer(&TaskQueue::consume, &queue);

    // Submit tasks from main thread
    submitBatch(queue, 20);

    // Wait for drain, then stop
    while (queue.size() > 0) {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    queue.stop();
    consumer.join();

    std::cout << "All tasks completed.\n";
    return 0;
}
