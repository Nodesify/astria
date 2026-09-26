/**
 * Repository layer for user persistence.
 *
 * Defines typed interfaces for data access and provides a concrete
 * implementation backed by an in-memory store.
 */

import { randomUUID } from "crypto";

/** Supported user roles in the system. */
export type UserRole = "admin" | "editor" | "viewer";

/** Shape of a persisted user record. */
export interface User {
  id: string;
  email: string;
  displayName: string;
  role: UserRole;
  createdAt: number;
  updatedAt: number;
}

/** Options when creating a new user. */
export interface CreateUserOptions {
  email: string;
  displayName: string;
  role?: UserRole;
}

/** Generic result wrapper for repository operations. */
export interface Result<T> {
  ok: boolean;
  value?: T;
  error?: string;
}

/**
 * Abstract definition of a user repository.
 *
 * WHY: Programming against an interface rather than a concrete class
 * makes it trivial to swap in a Postgres or Redis implementation in
 * production without touching the service layer.
 */
export interface IUserRepository {
  findById(id: string): Promise<Result<User>>;
  findByEmail(email: string): Promise<Result<User>>;
  create(options: CreateUserOptions): Promise<Result<User>>;
  update(id: string, patch: Partial<User>): Promise<Result<User>>;
  remove(id: string): Promise<Result<void>>;
}

/**
 * In-memory implementation of IUserRepository.
 *
 * Suitable for tests and local development. Not thread-safe.
 */
export class InMemoryUserRepository implements IUserRepository {
  private store: Map<string, User> = new Map();

  async findById(id: string): Promise<Result<User>> {
    const user = this.store.get(id);
    if (!user) return { ok: false, error: `User not found: ${id}` };
    return { ok: true, value: user };
  }

  async findByEmail(email: string): Promise<Result<User>> {
    for (const user of this.store.values()) {
      if (user.email === email) return { ok: true, value: user };
    }
    return { ok: false, error: `No user with email: ${email}` };
  }

  async create(options: CreateUserOptions): Promise<Result<User>> {
    const existing = await this.findByEmail(options.email);
    if (existing.ok) {
      return { ok: false, error: "Email already registered" };
    }
    const now = Date.now();
    // NOTE: randomUUID is used instead of a sequential ID to prevent
    // enumeration attacks on the public API.
    const user: User = {
      id: randomUUID(),
      email: options.email,
      displayName: options.displayName,
      role: options.role ?? "viewer",
      createdAt: now,
      updatedAt: now,
    };
    this.store.set(user.id, user);
    return { ok: true, value: user };
  }

  async update(id: string, patch: Partial<User>): Promise<Result<User>> {
    const existing = await this.findById(id);
    if (!existing.ok) return existing;
    const updated: User = { ...existing.value!, ...patch, updatedAt: Date.now() };
    this.store.set(id, updated);
    return { ok: true, value: updated };
  }

  async remove(id: string): Promise<Result<void>> {
    if (!this.store.delete(id)) {
      return { ok: false, error: `User not found: ${id}` };
    }
    return { ok: true };
  }
}

/**
 * Factory helper: pre-seed a repository with initial users.
 * @param repo - The repository instance to seed.
 * @param users - List of user creation options.
 */
export async function seedUsers(
  repo: IUserRepository,
  users: CreateUserOptions[],
): Promise<User[]> {
  const created: User[] = [];
  for (const opts of users) {
    const result = await repo.create(opts);
    if (result.ok && result.value) {
      created.push(result.value);
    }
  }
  return created;
}
