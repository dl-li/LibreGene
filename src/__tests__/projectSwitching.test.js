import { describe, expect, it, vi } from 'vitest';
import { createProjectSwitchQueue } from '../hooks/projectSwitching';

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('createProjectSwitchQueue', () => {
  it('serializes rapid switches and commits only the latest request', async () => {
    const b = deferred();
    const c = deferred();
    const calls = [];
    const commits = [];
    let running = 0;
    let maxRunning = 0;
    const queue = createProjectSwitchQueue({
      activate: vi.fn((id) => {
        calls.push(id);
        running++;
        maxRunning = Math.max(maxRunning, running);
        const result = id === 'B' ? b.promise : c.promise;
        return result.finally(() => running--);
      }),
      onCommit: (id) => commits.push(id),
    });

    const bSwitch = queue.switchTo('B');
    await Promise.resolve();
    const cSwitch = queue.switchTo('C');
    await Promise.resolve();
    expect(calls).toEqual(['B']);
    expect(commits).toEqual([]);

    b.resolve({ projects: [] });
    await bSwitch;
    expect(commits).toEqual([]);
    expect(calls).toEqual(['B', 'C']);
    c.resolve({ projects: [] });
    await cSwitch;

    expect(commits).toEqual(['C']);
    expect(maxRunning).toBe(1);
  });

  it('reports latest activation failure without committing a new workspace', async () => {
    const onCommit = vi.fn();
    const onError = vi.fn();
    const queue = createProjectSwitchQueue({
      activate: async () => ({ error: 'missing' }),
      onCommit,
      onError,
    });

    await queue.switchTo('B');
    expect(onCommit).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(expect.any(Error), 'B', expect.any(Function));
  });

  it('invalidates an in-progress failure recovery when a newer switch arrives', async () => {
    const recovery = deferred();
    let isCurrent;
    const queue = createProjectSwitchQueue({
      activate: async () => ({ error: 'missing' }),
      onError: async (_error, _id, current) => {
        isCurrent = current;
        await recovery.promise;
      },
    });
    const failed = queue.switchTo('B');
    await Promise.resolve();
    await Promise.resolve();
    queue.switchTo('C');
    expect(isCurrent()).toBe(false);
    recovery.resolve();
    await failed;
  });

  it('continues to the latest request after a stale request fails', async () => {
    const b = deferred();
    const commits = [];
    const queue = createProjectSwitchQueue({
      activate: (id) => (id === 'B' ? b.promise : Promise.resolve({})),
      onCommit: (id) => commits.push(id),
    });
    const bSwitch = queue.switchTo('B');
    await Promise.resolve();
    const cSwitch = queue.switchTo('C');
    b.reject(new Error('stale failure'));
    await Promise.all([bSwitch, cSwitch]);
    expect(commits).toEqual(['C']);
  });
});
