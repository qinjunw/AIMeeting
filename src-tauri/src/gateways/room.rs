use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRoom {
    pub room_id: String,
    pub match_code: String,
    pub participants: Vec<RoomParticipant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomParticipant {
    pub participant_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoomEvent {
    ParticipantJoined { participant: RoomParticipant },
    ParticipantLeft { participant_id: String },
    RoomClosed,
}

pub trait RoomEventSubscription: Send {
    fn next<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<RoomEvent>, RoomGatewayError>>;
}

pub trait RoomGateway: Send + Sync {
    fn create_room(&self) -> BoxFuture<'_, Result<MeetingRoom, RoomGatewayError>>;

    fn join_room<'a>(
        &'a self,
        match_code: &'a str,
    ) -> BoxFuture<'a, Result<MeetingRoom, RoomGatewayError>>;

    fn leave_room<'a>(&'a self, room_id: &'a str) -> BoxFuture<'a, Result<(), RoomGatewayError>>;

    fn subscribe_room_events<'a>(
        &'a self,
        room_id: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn RoomEventSubscription>, RoomGatewayError>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableRoomGateway;

impl RoomGateway for UnavailableRoomGateway {
    fn create_room(&self) -> BoxFuture<'_, Result<MeetingRoom, RoomGatewayError>> {
        Box::pin(async { Err(RoomGatewayError::Unavailable) })
    }

    fn join_room<'a>(
        &'a self,
        _match_code: &'a str,
    ) -> BoxFuture<'a, Result<MeetingRoom, RoomGatewayError>> {
        Box::pin(async { Err(RoomGatewayError::Unavailable) })
    }

    fn leave_room<'a>(&'a self, _room_id: &'a str) -> BoxFuture<'a, Result<(), RoomGatewayError>> {
        Box::pin(async { Err(RoomGatewayError::Unavailable) })
    }

    fn subscribe_room_events<'a>(
        &'a self,
        _room_id: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn RoomEventSubscription>, RoomGatewayError>> {
        Box::pin(async { Err(RoomGatewayError::Unavailable) })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RoomGatewayError {
    #[error("remote meeting rooms are not available in this release")]
    Unavailable,
    #[error("room request was rejected: {0}")]
    Rejected(String),
    #[error("room connection failed: {0}")]
    Connection(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unavailable_gateway_never_fakes_a_local_room() {
        let gateway = UnavailableRoomGateway;

        assert_eq!(
            gateway.create_room().await,
            Err(RoomGatewayError::Unavailable)
        );
        assert_eq!(
            gateway.join_room("123456").await,
            Err(RoomGatewayError::Unavailable)
        );
        assert_eq!(
            gateway.leave_room("room-1").await,
            Err(RoomGatewayError::Unavailable)
        );
        assert!(matches!(
            gateway.subscribe_room_events("room-1").await,
            Err(RoomGatewayError::Unavailable)
        ));
    }
}
