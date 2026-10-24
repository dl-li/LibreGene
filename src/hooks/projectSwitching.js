export function createProjectSwitchQueue({ activate, onCommit, onError }) {
  let latestRequest = 0;
  let tail = Promise.resolve();

  return {
    switchTo(id) {
      const request = ++latestRequest;
      const run = async () => {
        if (request !== latestRequest) return;
        try {
          const data = await activate(id);
          if (data?.error) throw new Error(data.error);
          if (request === latestRequest) onCommit(id, data);
        } catch (error) {
          if (request === latestRequest && onError) {
            await onError(error, id, () => request === latestRequest);
          }
        }
      };
      tail = tail.then(run, run);
      return tail;
    },
  };
}
