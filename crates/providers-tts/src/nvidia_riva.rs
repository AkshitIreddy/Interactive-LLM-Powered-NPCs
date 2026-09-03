//! Concrete NVIDIA NVCF/Riva gRPC transport for Magpie TTS.
//!
//! The protobuf inputs are pinned under `proto/` to NVIDIA Riva common
//! `r2.17.0` (`a7d342c`). Production construction always uses TLS and the
//! curated NVCF authority. Only explicit IP-loopback fixtures may use
//! plaintext transport.

use std::{fmt, time::Duration};

use async_trait::async_trait;
use tonic::{
    metadata::MetadataValue,
    transport::{Channel, ClientTlsConfig, Endpoint},
    Code, Request, Status,
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    NvidiaNimGrpcTransport, NvidiaNimSynthesisStream, NvidiaNvcfError, NvidiaSynthesizeFrame,
    NvidiaSynthesizeOnlineRequest, NVIDIA_MAGPIE_FUNCTION_ID, NVIDIA_MAGPIE_GRPC_AUTHORITY,
    RIVA_TTS_SYNTHESIZE_ONLINE_METHOD,
};

const NVIDIA_MAGPIE_GRPC_ORIGIN: &str = "https://grpc.nvcf.nvidia.com";
const MAX_GRPC_AUDIO_MESSAGE_BYTES: usize = 8 * 1_048_576;

mod proto {
    pub mod nvidia {
        pub mod riva {
            include!(concat!(env!("OUT_DIR"), "/nvidia.riva.rs"));

            pub mod tts {
                include!(concat!(env!("OUT_DIR"), "/nvidia.riva.tts.rs"));
            }
        }
    }
}

use proto::nvidia::riva::{
    tts::{
        riva_speech_synthesis_client::RivaSpeechSynthesisClient, SynthesizeSpeechRequest,
        SynthesizeSpeechResponse,
    },
    AudioEncoding, RequestId,
};

#[derive(Clone)]
pub struct TonicNvidiaNimGrpcTransport {
    channel: Channel,
    endpoint_kind: NvidiaGrpcEndpointKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NvidiaGrpcEndpointKind {
    HostedTls,
    LoopbackFixture,
}

impl fmt::Debug for TonicNvidiaNimGrpcTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TonicNvidiaNimGrpcTransport")
            .field("authority", &NVIDIA_MAGPIE_GRPC_AUTHORITY)
            .field("endpoint_kind", &self.endpoint_kind)
            .field("channel", &"<GRPC_CHANNEL>")
            .finish()
    }
}

impl TonicNvidiaNimGrpcTransport {
    /// Connects to NVIDIA's curated public NVCF gRPC gateway using TLS and
    /// WebPKI roots. The authority and certificate domain are not configurable.
    pub async fn connect() -> Result<Self, NvidiaNvcfError> {
        let endpoint = Endpoint::from_static(NVIDIA_MAGPIE_GRPC_ORIGIN)
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(120))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .tls_config(
                ClientTlsConfig::new()
                    .domain_name("grpc.nvcf.nvidia.com")
                    .with_webpki_roots(),
            )
            .map_err(|_| NvidiaNvcfError::Protocol)?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|_| NvidiaNvcfError::Unavailable)?;
        Ok(Self {
            channel,
            endpoint_kind: NvidiaGrpcEndpointKind::HostedTls,
        })
    }

    /// Connects to an IP-loopback plaintext gRPC server for contract tests.
    /// Hostnames, remote IPs, userinfo, paths, queries, and fragments fail
    /// closed so a production route cannot be redirected through this API.
    pub async fn connect_loopback_fixture(endpoint: Url) -> Result<Self, NvidiaNvcfError> {
        if endpoint.scheme() != "http"
            || !endpoint
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|host| host.is_loopback())
            || endpoint.username() != ""
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
        {
            return Err(NvidiaNvcfError::Protocol);
        }
        let endpoint = Endpoint::from_shared(endpoint.to_string())
            .map_err(|_| NvidiaNvcfError::Protocol)?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(120))
            .tcp_keepalive(Some(Duration::from_secs(30)));
        let channel = endpoint
            .connect()
            .await
            .map_err(|_| NvidiaNvcfError::Unavailable)?;
        Ok(Self {
            channel,
            endpoint_kind: NvidiaGrpcEndpointKind::LoopbackFixture,
        })
    }
}

#[async_trait]
impl NvidiaNimGrpcTransport for TonicNvidiaNimGrpcTransport {
    async fn synthesize_online(
        &self,
        request: NvidiaSynthesizeOnlineRequest,
    ) -> Result<Box<dyn NvidiaNimSynthesisStream>, NvidiaNvcfError> {
        if request.authority() != NVIDIA_MAGPIE_GRPC_AUTHORITY
            || !request.tls_required()
            || request.method() != RIVA_TTS_SYNTHESIZE_ONLINE_METHOD
            || request.function_id() != NVIDIA_MAGPIE_FUNCTION_ID
            || request.deadline() < Duration::from_millis(100)
            || request.deadline() > Duration::from_secs(120)
        {
            return Err(NvidiaNvcfError::Protocol);
        }
        let (scheme, credential) = request.authorization().expose_parts();
        if scheme != Some("Bearer") || credential.is_empty() {
            return Err(NvidiaNvcfError::Authentication);
        }
        let mut bearer = Zeroizing::new(Vec::with_capacity(7 + credential.len()));
        bearer.extend_from_slice(b"Bearer ");
        bearer.extend_from_slice(credential.as_bytes());
        let authorization = MetadataValue::try_from(bearer.as_slice())
            .map_err(|_| NvidiaNvcfError::Authentication)?;
        bearer.zeroize();

        let sample_rate_hz =
            i32::try_from(request.sample_rate_hz()).map_err(|_| NvidiaNvcfError::Protocol)?;
        let wire_request = SynthesizeSpeechRequest {
            text: request.text().expose().to_owned(),
            language_code: request.language_code().to_owned(),
            encoding: AudioEncoding::LinearPcm as i32,
            sample_rate_hz,
            voice_name: request.voice_name().to_owned(),
            zero_shot_data: None,
            custom_dictionary: String::new(),
            id: Some(RequestId {
                value: request.request_id().to_owned(),
            }),
        };
        let mut wire_request = Request::new(wire_request);
        wire_request.set_timeout(request.deadline());
        wire_request
            .metadata_mut()
            .insert("authorization", authorization);
        wire_request.metadata_mut().insert(
            "function-id",
            MetadataValue::from_static(NVIDIA_MAGPIE_FUNCTION_ID),
        );

        let mut client = RivaSpeechSynthesisClient::new(self.channel.clone())
            .max_decoding_message_size(MAX_GRPC_AUDIO_MESSAGE_BYTES)
            .max_encoding_message_size(MAX_GRPC_AUDIO_MESSAGE_BYTES);
        let stream = client
            .synthesize_online(wire_request)
            .await
            .map_err(map_status)?
            .into_inner();
        Ok(Box::new(TonicNvidiaSynthesisStream {
            stream: Some(stream),
            cancelled: false,
        }))
    }
}

struct TonicNvidiaSynthesisStream {
    stream: Option<tonic::Streaming<SynthesizeSpeechResponse>>,
    cancelled: bool,
}

#[async_trait]
impl NvidiaNimSynthesisStream for TonicNvidiaSynthesisStream {
    async fn next_frame(&mut self) -> Option<Result<NvidiaSynthesizeFrame, NvidiaNvcfError>> {
        if self.cancelled {
            return Some(Err(NvidiaNvcfError::Cancelled));
        }
        let stream = self.stream.as_mut()?;
        match stream.message().await {
            Ok(Some(response)) => Some(Ok(NvidiaSynthesizeFrame {
                pcm: response.audio,
                // The pinned Riva metadata exposes token-duration estimates,
                // not reliable word spans. Never synthesize alignment guesses.
                word_offsets: Vec::new(),
            })),
            Ok(None) => {
                self.stream = None;
                None
            }
            Err(status) => {
                self.stream = None;
                Some(Err(map_status(status)))
            }
        }
    }

    async fn cancel(&mut self) -> Result<(), NvidiaNvcfError> {
        self.cancelled = true;
        // Dropping tonic::Streaming drops the HTTP/2 response body, which sends
        // cancellation/reset to the peer without waiting for another frame.
        self.stream = None;
        Ok(())
    }
}

fn map_status(status: Status) -> NvidiaNvcfError {
    match status.code() {
        Code::Unauthenticated | Code::PermissionDenied => NvidiaNvcfError::Authentication,
        Code::ResourceExhausted => NvidiaNvcfError::RateLimited {
            retry_after: retry_after(status.metadata()),
        },
        Code::DeadlineExceeded => NvidiaNvcfError::DeadlineExceeded,
        Code::NotFound => NvidiaNvcfError::BadFunction,
        Code::Unavailable | Code::Internal | Code::Unknown => NvidiaNvcfError::Unavailable,
        Code::Cancelled => NvidiaNvcfError::Cancelled,
        _ => NvidiaNvcfError::Protocol,
    }
}

fn retry_after(metadata: &tonic::metadata::MetadataMap) -> Option<Duration> {
    metadata
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        pin::Pin,
        sync::{Arc, Mutex},
    };

    use futures_util::Stream;
    use tokio::sync::oneshot;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{transport::Server, Response};

    use super::{
        proto::nvidia::riva::tts::{
            riva_speech_synthesis_server::{RivaSpeechSynthesis, RivaSpeechSynthesisServer},
            RivaSynthesisConfigRequest, RivaSynthesisConfigResponse, SynthesizeSpeechRequest,
            SynthesizeSpeechResponse,
        },
        *,
    };
    use crate::{
        AudioFormat, CredentialResolveError, HostedTtsProviderId, MockNvidiaHttpTransport,
        NvidiaNimMagpie, NvidiaStockVoice, NvidiaVoiceOrigin, PcmChunk, PcmEncoding,
        ProviderCredentialResolver, SemanticClausePolicy, SensitiveString, SessionIdentity,
        StreamingTtsProvider, TtsErrorKind, TtsEvent, TtsSessionRequest, VoiceBinding,
        VoiceBindings, NVIDIA_MAGPIE_MODEL_ID,
    };

    const FIXTURE_CREDENTIAL: &str = "fixture-nvidia-grpc-credential";
    const FIXTURE_AUTHORIZATION: &str = "Bearer fixture-nvidia-grpc-credential";

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CapturedRequest {
        authorization_valid: bool,
        function_id: Option<String>,
        text: String,
        language_code: String,
        encoding: i32,
        sample_rate_hz: i32,
        voice_name: String,
        request_id: Option<String>,
        zero_shot_present: bool,
        custom_dictionary: String,
    }

    #[derive(Clone, Copy)]
    enum FixtureBehavior {
        TwoFrames,
        Pending,
    }

    struct FixtureService {
        behavior: FixtureBehavior,
        captured: Arc<Mutex<Option<CapturedRequest>>>,
    }

    #[tonic::async_trait]
    impl RivaSpeechSynthesis for FixtureService {
        async fn synthesize(
            &self,
            _: Request<SynthesizeSpeechRequest>,
        ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
            Err(Status::unimplemented("fixture unary route disabled"))
        }

        type SynthesizeOnlineStream =
            Pin<Box<dyn Stream<Item = Result<SynthesizeSpeechResponse, Status>> + Send>>;

        async fn synthesize_online(
            &self,
            request: Request<SynthesizeSpeechRequest>,
        ) -> Result<Response<Self::SynthesizeOnlineStream>, Status> {
            let authorization_valid = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                == Some(FIXTURE_AUTHORIZATION);
            let function_id = request
                .metadata()
                .get("function-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let request = request.into_inner();
            *self.captured.lock().expect("capture lock") = Some(CapturedRequest {
                authorization_valid,
                function_id,
                text: request.text,
                language_code: request.language_code,
                encoding: request.encoding,
                sample_rate_hz: request.sample_rate_hz,
                voice_name: request.voice_name,
                request_id: request.id.map(|value| value.value),
                zero_shot_present: request.zero_shot_data.is_some(),
                custom_dictionary: request.custom_dictionary,
            });
            let stream: Self::SynthesizeOnlineStream = match self.behavior {
                FixtureBehavior::TwoFrames => Box::pin(tokio_stream::iter([
                    Ok(SynthesizeSpeechResponse {
                        audio: vec![1, 2, 3, 4],
                        meta: None,
                        id: None,
                    }),
                    Ok(SynthesizeSpeechResponse {
                        audio: vec![5, 6],
                        meta: None,
                        id: None,
                    }),
                ])),
                FixtureBehavior::Pending => Box::pin(futures_util::stream::pending()),
            };
            Ok(Response::new(stream))
        }

        async fn get_riva_synthesis_config(
            &self,
            _: Request<RivaSynthesisConfigRequest>,
        ) -> Result<Response<RivaSynthesisConfigResponse>, Status> {
            Err(Status::unimplemented("fixture config route disabled"))
        }
    }

    struct FixtureCredential;

    #[async_trait]
    impl ProviderCredentialResolver for FixtureCredential {
        async fn resolve(
            &self,
            provider_id: HostedTtsProviderId,
        ) -> Result<SensitiveString, CredentialResolveError> {
            assert_eq!(provider_id, HostedTtsProviderId::NvidiaNimMagpie);
            Ok(SensitiveString::new(FIXTURE_CREDENTIAL))
        }
    }

    fn voice() -> NvidiaStockVoice {
        NvidiaStockVoice {
            id: "Magpie-Multilingual.EN-US.Aria".to_owned(),
            display_name: "Aria".to_owned(),
            locale: "en-US".to_owned(),
            origin: NvidiaVoiceOrigin::ProviderStock,
        }
    }

    fn session_request() -> TtsSessionRequest {
        TtsSessionRequest {
            identity: SessionIdentity {
                session_id: "grpc-session".to_owned(),
                turn_id: "grpc-turn".to_owned(),
                cancellation_generation: 3,
            },
            locale: "en-US".to_owned(),
            voice_intent_id: "companion".to_owned(),
            output: AudioFormat {
                encoding: PcmEncoding::PcmS16Le,
                sample_rate_hz: 22_050,
                channels: 1,
            },
            request_alignment: false,
            request_visemes: false,
            clause_policy: SemanticClausePolicy::default(),
        }
    }

    async fn provider_fixture(
        behavior: FixtureBehavior,
        deadline: Duration,
    ) -> (
        NvidiaNimMagpie,
        Arc<Mutex<Option<CapturedRequest>>>,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind gRPC fixture");
        let address = listener.local_addr().expect("fixture address");
        let captured = Arc::new(Mutex::new(None));
        let service = FixtureService {
            behavior,
            captured: Arc::clone(&captured),
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(RivaSpeechSynthesisServer::new(service))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ignored = shutdown_rx.await;
                })
                .await
                .expect("serve gRPC fixture");
        });
        let endpoint = Url::parse(&format!("http://{address}/")).expect("fixture URL");
        let transport = TonicNvidiaNimGrpcTransport::connect_loopback_fixture(endpoint)
            .await
            .expect("connect fixture");
        let binding = VoiceBinding {
            intent_id: "companion".to_owned(),
            provider_id: HostedTtsProviderId::NvidiaNimMagpie,
            voice_id: voice().id,
            model_id: NVIDIA_MAGPIE_MODEL_ID.to_owned(),
            provider_options: BTreeMap::new(),
        };
        let mut provider = NvidiaNimMagpie::new(
            Arc::new(transport),
            Arc::new(MockNvidiaHttpTransport::returning(Ok(vec![voice()]))),
            Arc::new(FixtureCredential),
            VoiceBindings::new([binding]).expect("fixture binding"),
        );
        provider
            .set_request_deadline(deadline)
            .expect("fixture deadline");
        provider
            .discover_stock_voices()
            .await
            .expect("fixture voice discovery");
        (provider, captured, shutdown_tx, server)
    }

    #[tokio::test]
    async fn concrete_grpc_transport_interoperates_and_preserves_curated_wire_contract() {
        let (provider, captured, shutdown, server) =
            provider_fixture(FixtureBehavior::TwoFrames, Duration::from_secs(2)).await;
        let mut session = provider
            .start_session(session_request())
            .await
            .expect("start session");
        session
            .push_text("The north beacon is ready.")
            .await
            .expect("start concrete stream");
        session.finish().await.expect("finish");
        assert!(matches!(
            session.next_event().await,
            Some(Ok(TtsEvent::Audio(PcmChunk { data, .. }))) if data == vec![1, 2, 3, 4]
        ));
        assert!(matches!(
            session.next_event().await,
            Some(Ok(TtsEvent::Audio(PcmChunk { data, .. }))) if data == vec![5, 6]
        ));
        assert_eq!(session.next_event().await, Some(Ok(TtsEvent::Completed)));

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("request");
        assert!(captured.authorization_valid);
        assert_eq!(
            captured.function_id.as_deref(),
            Some(NVIDIA_MAGPIE_FUNCTION_ID)
        );
        assert_eq!(captured.text, "The north beacon is ready.");
        assert_eq!(captured.language_code, "en-US");
        assert_eq!(captured.encoding, AudioEncoding::LinearPcm as i32);
        assert_eq!(captured.sample_rate_hz, 22_050);
        assert_eq!(captured.voice_name, "Magpie-Multilingual.EN-US.Aria");
        assert_eq!(
            captured.request_id.as_deref(),
            Some("grpc-session:grpc-turn:3:0")
        );
        assert!(!captured.zero_shot_present);
        assert!(captured.custom_dictionary.is_empty());
        let _ignored = shutdown.send(());
        server.await.expect("fixture server");
    }

    #[tokio::test]
    async fn concrete_grpc_stream_deadline_and_cancellation_are_bounded() {
        let (provider, _, shutdown, server) =
            provider_fixture(FixtureBehavior::Pending, Duration::from_millis(100)).await;
        let mut session = provider
            .start_session(session_request())
            .await
            .expect("start session");
        session
            .push_text("The north beacon is ready.")
            .await
            .expect("start pending stream");
        session.finish().await.expect("finish");
        let error = tokio::time::timeout(Duration::from_secs(1), session.next_event())
            .await
            .expect("outer deadline")
            .expect("timeout event")
            .expect_err("pending stream must time out");
        assert_eq!(error.kind, TtsErrorKind::Timeout);
        assert_eq!(error.code, "nvidia_deadline_exceeded");

        drop(session);
        let _ignored = shutdown.send(());
        server.await.expect("fixture server");

        let (provider, _, shutdown, server) =
            provider_fixture(FixtureBehavior::Pending, Duration::from_secs(2)).await;
        let mut session = provider
            .start_session(session_request())
            .await
            .expect("start session");
        session
            .push_text("The north beacon is ready.")
            .await
            .expect("start pending stream");
        tokio::time::timeout(Duration::from_millis(250), session.cancel())
            .await
            .expect("bounded cancel")
            .expect("cancel stream");
        assert_eq!(
            session.next_event().await,
            Some(Ok(TtsEvent::Interrupted { reason: "barge_in" }))
        );

        let _ignored = shutdown.send(());
        server.await.expect("fixture server");
    }

    #[tokio::test]
    async fn grpc_fixture_constructor_rejects_non_loopback_endpoints() {
        let error = TonicNvidiaNimGrpcTransport::connect_loopback_fixture(
            Url::parse("http://example.com/").expect("URL"),
        )
        .await
        .expect_err("remote plaintext rejected");
        assert_eq!(error, NvidiaNvcfError::Protocol);
    }
}
