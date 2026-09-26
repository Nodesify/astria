/**
 * Task scheduler for background job processing.
 *
 * Provides a lightweight priority queue that executes async tasks
 * according to their scheduled time and priority level.
 */

const { EventEmitter } = require("events");
const { v4: uuidv4 } = require("uuid");

/**
 * Represents a unit of work to be scheduled.
 */
class Task {
  /**
   * @param {string} name - Human-readable task identifier.
   * @param {Function} handler - Async function to execute.
   * @param {object} [options] - Scheduling options.
   * @param {number} [options.priority=5] - Priority 1 (highest) to 10 (lowest).
   * @param {number} [options.delay=0] - Milliseconds before the task becomes runnable.
   */
  constructor(name, handler, options = {}) {
    this.id = uuidv4();
    this.name = name;
    this.handler = handler;
    this.priority = options.priority ?? 5;
    this.delay = options.delay ?? 0;
    this.createdAt = Date.now();
    this.resolved = false;
  }

  /** Return true when the delay period has elapsed. */
  isReady() {
    return Date.now() - this.createdAt >= this.delay;
  }

  /** Execute the underlying handler and mark this task resolved. */
  async run() {
    const result = await this.handler();
    this.resolved = true;
    return result;
  }
}

/**
 * Priority-based task scheduler built on top of EventEmitter.
 *
 * Emits "completed" after each successful task and "error" on failure.
 */
class TaskScheduler extends EventEmitter {
  constructor() {
    super();
    this._queue = [];
    this._running = false;
  }

  /**
   * Enqueue a new task.
   * @param {Task} task
   */
  enqueue(task) {
    // NOTE: Insertion sort keeps the queue ordered by priority so
    // dequeue() is always O(1). Fine for small queues (< 10k tasks).
    let inserted = false;
    for (let i = 0; i < this._queue.length; i++) {
      if (task.priority < this._queue[i].priority) {
        this._queue.splice(i, 0, task);
        inserted = true;
        break;
      }
    }
    if (!inserted) {
      this._queue.push(task);
    }
  }

  /** Remove and return the highest-priority ready task, or null. */
  dequeue() {
    for (let i = 0; i < this._queue.length; i++) {
      if (this._queue[i].isReady()) {
        return this._queue.splice(i, 1)[0];
      }
    }
    return null;
  }

  /**
   * Run the scheduler loop until the queue is empty.
   * WHY: We process one task at a time to avoid thundering-herd effects
   * on downstream services that share a rate-limited API quota.
   */
  async runAll() {
    this._running = true;
    while (this._running) {
      const task = this.dequeue();
      if (!task) break;
      try {
        const result = await task.run();
        this.emit("completed", { task, result });
      } catch (err) {
        this.emit("error", { task, error: err });
      }
    }
    this._running = false;
  }

  /** Signal the scheduler to stop after the current task. */
  stop() {
    this._running = false;
  }
}

/**
 * Helper: create and schedule a task in one call.
 * @param {TaskScheduler} scheduler
 * @param {string} name
 * @param {Function} handler
 * @param {object} [options]
 * @returns {Task}
 */
function scheduleTask(scheduler, name, handler, options) {
  const task = new Task(name, handler, options);
  scheduler.enqueue(task);
  return task;
}

module.exports = { Task, TaskScheduler, scheduleTask };
