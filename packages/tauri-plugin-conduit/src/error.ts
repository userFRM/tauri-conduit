/** Structured error from the conduit transport layer. */
export class ConduitError extends Error {
  /** HTTP status code from the conduit protocol response. */
  readonly status: number;
  /** The command or channel that caused the error. */
  readonly target: string;

  constructor(status: number, target: string, message: string) {
    super(message);
    Object.setPrototypeOf(this, new.target.prototype);
    this.name = 'ConduitError';
    this.status = status;
    this.target = target;
  }
}
