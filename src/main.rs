#![forbid(unsafe_code)]
#![deny(clippy::all)]
// clippy::pedantic applied per-module; generated proto code is exempt.

use tonic::{transport::Server, Request, Response, Status, Code};
use risk_engine::risk_service_server::{RiskService, RiskServiceServer};
use risk_engine::{RiskRequest, RiskResponse};
use crate::risk::{
    decode_order, decode_portfolio, InternalOrder, InternalPortfolio, RiskError, RiskParams,
    RiskResult,
};
use std::time::Duration;

#[allow(clippy::pedantic, clippy::missing_errors_doc, clippy::doc_markdown, clippy::default_trait_access)]
pub mod risk_engine {
    tonic::include_proto!("risk_engine");
}

pub mod risk;

// TigerStyle: typed error to gRPC status mapping.
// Each error variant maps to a specific gRPC status code.
impl From<RiskError> for Status {
    fn from(err: RiskError) -> Self {
        match err {
            RiskError::RequestIdEmpty
            | RiskError::BalanceZero
            | RiskError::EntryPriceZero
            | RiskError::StopDistanceZero
            | RiskError::InvalidVolatility
            | RiskError::PortfolioDecodeError
            | RiskError::OrderDecodeError => Status::new(Code::InvalidArgument, err.to_string()),
            RiskError::RiskThresholdViolation => {
                Status::new(Code::FailedPrecondition, err.to_string())
            }
        }
    }
}

// Boundary layer: domain mapping with airgap between gRPC and pure math.
impl TryFrom<RiskRequest> for (InternalPortfolio, InternalOrder, String) {
    type Error = Status;

    fn try_from(req: RiskRequest) -> Result<Self, Self::Error> {
        if req.request_id.is_empty() {
            return Err(RiskError::RequestIdEmpty.into());
        }

        // Decode: Map byte payloads to mathematical domain.
        // TigerStyle: reject unknown formats instead of silently substituting data.
        let portfolio = decode_portfolio(&req.portfolio_state)?;
        let mut order = decode_order(&req.order_details)?;
        order.asset_volatility = req.asset_volatility;

        Ok((portfolio, order, req.request_id))
    }
}

impl From<RiskResult> for RiskResponse {
    fn from(res: RiskResult) -> Self {
        // Pair assertion: is_approved must be consistent with size > 0.
        // Enforced here AND in risk::evaluate postcondition.
        debug_assert_eq!(res.is_approved, res.size > 0.0);
        Self {
            request_id: res.request_id,
            is_approved: res.is_approved,
            kelly_adjusted_size: res.size,
            rejection_reason: res.reason.map_or_else(String::new, |e| e.to_string()),
        }
    }
}

#[derive(Debug, Default)]
pub struct RaasRiskService {}

#[tonic::async_trait]
impl RiskService for RaasRiskService {
    async fn validate_order(
        &self,
        request: Request<RiskRequest>,
    ) -> Result<Response<RiskResponse>, Status> {
        // PHASE 1: DECODE (Boundary -> Internal)
        let (portfolio, order, request_id) = request.into_inner().try_into()?;

        // System params (Injected from config in production)
        let params = RiskParams {
            risk_percent: 2.0,
            target_vol: 1000.0,
            max_position_cap: 1.0,
        };

        // PHASE 2: COMPUTE (Pure Logic)
        let result = risk::evaluate(&portfolio, &order, &params, request_id);

        // Pair assertion: result invariants checked before encoding.
        debug_assert!(result.size.is_finite());
        debug_assert!(result.size >= 0.0);
        debug_assert_eq!(result.is_approved, result.size > 0.0);

        // PHASE 3: ENCODE (Internal -> Boundary)
        Ok(Response::new(result.into()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse()?;
    let risk_service = RaasRiskService::default();

    println!("RaaS Boundary active. Listening on {addr}");

    // TigerStyle: explicit resource limits — no unbounded defaults.
    Server::builder()
        .max_connection_age(Duration::from_secs(300))
        .max_concurrent_streams(256)
        .add_service(RiskServiceServer::new(risk_service))
        .serve(addr)
        .await?;

    Ok(())
}
