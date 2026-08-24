const MAX_HISTORY = 50;

/**
 * 撤销/重做历史栈
 *
 * 快照格式: { sequence, features, primers, cursorIndex, selStart, selEnd }
 *
 * 刚打开文件时: reset() 初始化第一条记录
 * 每次编辑成功后: push() 记录编辑后的新状态（undo 退回前一条 = 编辑前状态，
 * redo 前进到本条 = 重做该编辑）
 */
export function createEditHistory() {
  const stack = [];
  const listeners = new Set();
  let cursor = -1;

  const notify = () => listeners.forEach((fn) => fn());

  return {
    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },

    reset(snapshot) {
      stack.length = 0;
      stack.push({ ...snapshot });
      cursor = 0;
      notify();
    },

    /**
     * 记录一个新状态（编辑后）。
     * 如果当前不在栈顶（即 undo 之后），截断之后的 redo 条目。
     */
    push(snapshot) {
      // 截断 cursor 之后的条目
      stack.splice(cursor + 1);
      stack.push({ ...snapshot });
      if (stack.length > MAX_HISTORY) {
        stack.shift();
      }
      cursor = stack.length - 1;
      notify();
    },

    /** 返回前一个状态的快照（拷贝），无历史时返回 null */
    undo() {
      if (cursor <= 0) return null;
      cursor--;
      notify();
      return { ...stack[cursor] };
    },

    /** 返回后一个状态的快照（拷贝），无可重做时返回 null */
    redo() {
      if (cursor >= stack.length - 1) return null;
      cursor++;
      notify();
      return { ...stack[cursor] };
    },

    canUndo() {
      return cursor > 0;
    },

    canRedo() {
      return cursor < stack.length - 1;
    },

    /** 当前状态索引（调试用） */
    getCursor() {
      return cursor;
    },

    /** 当前栈长度（调试用） */
    getLength() {
      return stack.length;
    },
  };
}
