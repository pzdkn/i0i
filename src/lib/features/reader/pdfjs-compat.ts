type ReadableStreamPrototype = ReadableStream<unknown> & {
  values?: () => AsyncIterableIterator<unknown>;
  [Symbol.asyncIterator]?: () => AsyncIterableIterator<unknown>;
};

type PromiseWithResolvers<T> = {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
};

type PromiseConstructorWithResolvers = PromiseConstructor & {
  withResolvers?: <T>() => PromiseWithResolvers<T>;
};

export function ensurePdfJsRuntimeCompatibility() {
  return {
    readableStream: ensureReadableStreamAsyncIterable(),
    promiseWithResolvers: ensurePromiseWithResolvers(),
  };
}

function ensureReadableStreamAsyncIterable() {
  if (typeof ReadableStream === "undefined") {
    return {
      hasReadableStream: false,
      hasGetReader: false,
      hasValues: false,
      hasAsyncIterator: false,
      installed: false,
    };
  }

  const prototype = ReadableStream.prototype as ReadableStreamPrototype;
  const hasValues = typeof prototype.values === "function";
  const hasAsyncIterator = typeof prototype[Symbol.asyncIterator] === "function";

  if (hasValues && hasAsyncIterator) {
    return {
      hasReadableStream: true,
      hasGetReader: typeof prototype.getReader === "function",
      hasValues: true,
      hasAsyncIterator: true,
      installed: false,
    };
  }

  const values = async function* values(this: ReadableStream<unknown>) {
    const reader = this.getReader();

    try {
      while (true) {
        const result = await reader.read();
        if (result.done) {
          return;
        }

        yield result.value;
      }
    } finally {
      reader.releaseLock();
    }
  };

  if (!hasValues) {
    Object.defineProperty(prototype, "values", {
      configurable: true,
      writable: true,
      value: values,
    });
  }

  if (!hasAsyncIterator) {
    Object.defineProperty(prototype, Symbol.asyncIterator, {
      configurable: true,
      writable: true,
      value: prototype.values ?? values,
    });
  }

  return {
    hasReadableStream: true,
    hasGetReader: typeof prototype.getReader === "function",
    hasValues: typeof prototype.values === "function",
    hasAsyncIterator: typeof prototype[Symbol.asyncIterator] === "function",
    installed: true,
  };
}

function ensurePromiseWithResolvers() {
  const promiseConstructor = Promise as PromiseConstructorWithResolvers;

  if (typeof promiseConstructor.withResolvers === "function") {
    return {
      hasWithResolvers: true,
      installed: false,
    };
  }

  promiseConstructor.withResolvers = function withResolvers<T>() {
    let resolve!: (value: T | PromiseLike<T>) => void;
    let reject!: (reason?: unknown) => void;
    const promise = new Promise<T>((nextResolve, nextReject) => {
      resolve = nextResolve;
      reject = nextReject;
    });

    return { promise, resolve, reject };
  };

  return {
    hasWithResolvers: typeof promiseConstructor.withResolvers === "function",
    installed: true,
  };
}
