# testudo-raas (Risk-as-a-Service)

_`testudo-raas` is a gRPC microservice that answers one question: "Given my portfolio and this proposed trade,
what's the safe position size?" It runs three mathematical models (Kelly Criterion, Fixed Fractional, Volatility-Adjusted) and picks
the most conservative answer. It's stateless, validated, fuzz-tested, and deployable as a distroless container.
The core capability: mathematical position sizing as a service — it tells you how much to bet, not what to bet on._

## TECHNICAL SPECIFICATION
- **Runtime:** Rust (Edition 2024).
- **Interface:** gRPC over HTTP/2 (Tonic/Prost).
- **Concurrency:** Asynchronous I/O via Tokio.
- **Architecture:** Pure functional core isolated from network/IO via `TryFrom` boundary mapping.
- **Logic:** 
  - **Sizing:** Kelly Criterion (Half-Kelly) & Fixed Fractional.
  - **Adjustments:** Inverse Volatility Scaling (ATR-based).
  - **Execution:** Conservative Wins (Smallest size prevails).

## SIMPLE EXPLANATION
`testudo-raas` is a specialized "math engine" used by the trading system. 
1. **The Input:** It receives an account's balance and a proposed trade.
2. **The Calculation:** It runs the trade through several mathematical models to determine if it's too risky based on current market volatility.
3. **The Result:** It tells the system exactly how much of the asset to buy/sell to maximize growth while preventing catastrophic losses.

## DEPLOYMENT
Built for edge deployment via distroless containers.
```bash
podman build -t testudo-raas:latest .
podman run -p 50051:50051 testudo-raas:latest
```
