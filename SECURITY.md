# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in tauri-conduit, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: [security contact -- use GitHub's private vulnerability reporting]

Or use GitHub's [private vulnerability reporting](https://github.com/userFRM/tauri-conduit/security/advisories/new) feature.

## Security Model

tauri-conduit runs entirely in-process within a Tauri v2 application. There is no network surface.

### Threat Model

- **In scope:** Memory safety, authentication bypass, side-channel attacks on invoke key validation, buffer overflows in codec/ring buffer, denial of service via malformed frames.
- **Out of scope:** Attacks requiring code execution in the same process (the attacker already has full access), physical access, webview sandbox escapes (those are Tauri/platform bugs).

### Security Design

- **Per-launch invoke key:** 32 random bytes (via `getrandom`), hex-encoded, validated with constant-time comparison (`subtle` crate) on every custom protocol request.
- **No network surface:** All communication runs in the same address space via Tauri's custom protocol handler. No ports, no sockets, no endpoints.
- **Flat invoke key (no per-command ACL):** Any webview with the invoke key can call any registered command. This is simpler than Tauri's per-command capability system. For granular access control, use Tauri's built-in IPC.
- **CSP compliance:** Custom protocol handler does not require Content Security Policy exceptions.

## Supported Versions

| Version | Supported |
|---------|-----------|
| 2.0.x   | Yes       |
| 1.0.x   | Security fixes only |
| 0.1.x   | No        |
