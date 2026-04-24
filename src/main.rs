use tonic::{transport::Server, Request, Response, Status};
use risk_engine::risk_service_server::{RiskService, RiskServiceServer};
use risk_engine::{RiskRequest, RiskResponse};
use crate::risk::{InternalPortfolio, InternalOrder, RiskParams, RiskResult};

pub mod risk_engine {
    tonic::include_proto!("risk_engine");
}

pub mod risk;

// BOUNDARY LAYER: DOMAIN MAPPING (AIRGAP)
impl TryFrom<RiskRequest> for (InternalPortfolio, InternalOrder, String) {
    type Error = Status;

    fn try_from(req: RiskRequest) -> Result<Self, Self::Error> {
        if req.request_id.is_empty() {
            return Err(Status::invalid_argument("MISSING_REQUEST_ID"));
        }

        // DECODE: Mapping byte payloads to mathematical domain
        // Placeholder values used for bytes extraction until schema is finalized
        let portfolio = InternalPortfolio {
            balance: 10000.0,
            win_rate: 0.55,
            avg_win: 1.5,
            avg_loss: 1.0,
        };

        let order = InternalOrder {
            entry_price: 50000.0,
            stop_price: 49000.0,
            asset_volatility: req.asset_volatility,
        };

        Ok((portfolio, order, req.request_id))
    }
}

impl From<RiskResult> for RiskResponse {
    fn from(res: RiskResult) -> Self {
        Self {
            request_id: res.request_id,
            is_approved: res.is_approved,
            kelly_adjusted_size: res.size,
            rejection_reason: res.reason,
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

        // PHASE 3: ENCODE (Internal -> Boundary)
        Ok(Response::new(result.into()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:50051".parse()?;
    let risk_service = RaasRiskService::default();

    println!("RaaS Boundary active. Listening on {}", addr);

    Server::builder()
        .add_service(RiskServiceServer::new(risk_service))
        .serve(addr)
        .await?;

    Ok(())
}
