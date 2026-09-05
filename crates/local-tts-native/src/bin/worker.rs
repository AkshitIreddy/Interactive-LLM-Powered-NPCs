#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

#[cfg(not(windows))]
fn main() {
    eprintln!("npc-local-tts-native-worker is Windows x64 only");
    std::process::exit(2);
}

#[cfg(windows)]
mod windows_worker {
    use std::{
        env,
        ffi::{c_char, c_float, c_void, CStr, CString},
        fs::File,
        io::{self, BufReader, BufWriter},
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::{self, Receiver, SyncSender},
            Arc, Mutex,
        },
    };

    use libloading::Library;
    use npc_local_tts_native::wire::{
        read_command_blocking, write_blocking, WorkerCommand, WorkerEvent,
    };
    use sha2::{Digest, Sha256};

    const PACK_ID: &str = "local.tts.kokoro-v1.0-int8.sherpa-onnx-cpu.windows-x64";
    const REVISION: &str = "2026.08.30-r1";
    const SAMPLE_RATE: u32 = 24_000;
    const VOICES: [&str; 28] = [
        "af_alloy",
        "af_aoede",
        "af_bella",
        "af_heart",
        "af_jessica",
        "af_kore",
        "af_nicole",
        "af_nova",
        "af_river",
        "af_sarah",
        "af_sky",
        "am_adam",
        "am_echo",
        "am_eric",
        "am_fenrir",
        "am_liam",
        "am_michael",
        "am_onyx",
        "am_puck",
        "am_santa",
        "bf_alice",
        "bf_emma",
        "bf_isabella",
        "bf_lily",
        "bm_daniel",
        "bm_fable",
        "bm_george",
        "bm_lewis",
    ];
    const CRITICAL: [(&str, u64, &str); 6] = [
        (
            "model/model.int8.onnx",
            114_298_054,
            "77ef4f0513401d508ed7831f8504c7042df58bc75e004ec9666894590f999b1d",
        ),
        (
            "model/voices.bin",
            27_678_720,
            "8a77c0d397026208d22211f37670b5b3b11e03f190756b25a1d24041fced82a9",
        ),
        (
            "model/tokens.txt",
            687,
            "6ebb6bb288f20f3ae8d004d3c2ca27697da27c037d75e81a60e2a6a663f95425",
        ),
        (
            "model/lexicon-us-en.txt",
            5_956_885,
            "7daaab53a181be9885b853a8582bf1838186317e5dadacbcef9c426d6fa0da14",
        ),
        (
            "model/espeak-ng-data/phondata",
            550_424,
            "4e0288957874029a8c3c9f41a8f517ad4bf18127046decbdd4b9d1d6807ce3a3",
        ),
        (
            "model/LICENSE",
            11_358,
            "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30",
        ),
    ];

    #[repr(C)]
    struct Vits {
        model: *const c_char,
        lexicon: *const c_char,
        tokens: *const c_char,
        data_dir: *const c_char,
        noise_scale: c_float,
        noise_scale_w: c_float,
        length_scale: c_float,
        dict_dir: *const c_char,
    }
    #[repr(C)]
    struct Matcha {
        acoustic_model: *const c_char,
        vocoder: *const c_char,
        lexicon: *const c_char,
        tokens: *const c_char,
        data_dir: *const c_char,
        noise_scale: c_float,
        length_scale: c_float,
        dict_dir: *const c_char,
    }
    #[repr(C)]
    struct Kokoro {
        model: *const c_char,
        voices: *const c_char,
        tokens: *const c_char,
        data_dir: *const c_char,
        length_scale: c_float,
        dict_dir: *const c_char,
        lexicon: *const c_char,
        lang: *const c_char,
    }
    #[repr(C)]
    struct Kitten {
        model: *const c_char,
        voices: *const c_char,
        tokens: *const c_char,
        data_dir: *const c_char,
        length_scale: c_float,
    }
    #[repr(C)]
    struct Zipvoice {
        tokens: *const c_char,
        encoder: *const c_char,
        decoder: *const c_char,
        vocoder: *const c_char,
        data_dir: *const c_char,
        lexicon: *const c_char,
        feat_scale: c_float,
        t_shift: c_float,
        target_rms: c_float,
        guidance_scale: c_float,
    }
    #[repr(C)]
    struct Pocket {
        lm_flow: *const c_char,
        lm_main: *const c_char,
        encoder: *const c_char,
        decoder: *const c_char,
        text_conditioner: *const c_char,
        vocab_json: *const c_char,
        token_scores_json: *const c_char,
        voice_embedding_cache_capacity: i32,
    }
    #[repr(C)]
    struct Supertonic {
        duration_predictor: *const c_char,
        text_encoder: *const c_char,
        vector_estimator: *const c_char,
        vocoder: *const c_char,
        tts_json: *const c_char,
        unicode_indexer: *const c_char,
        voice_style: *const c_char,
    }
    #[repr(C)]
    struct ModelConfig {
        vits: Vits,
        num_threads: i32,
        debug: i32,
        provider: *const c_char,
        matcha: Matcha,
        kokoro: Kokoro,
        kitten: Kitten,
        zipvoice: Zipvoice,
        pocket: Pocket,
        supertonic: Supertonic,
    }
    #[repr(C)]
    struct TtsConfig {
        model: ModelConfig,
        rule_fsts: *const c_char,
        max_num_sentences: i32,
        rule_fars: *const c_char,
        silence_scale: c_float,
    }
    #[repr(C)]
    struct GenerationConfig {
        silence_scale: c_float,
        speed: c_float,
        sid: i32,
        reference_audio: *const c_float,
        reference_audio_len: i32,
        reference_sample_rate: i32,
        reference_text: *const c_char,
        num_steps: i32,
        extra: *const c_char,
    }
    #[repr(C)]
    struct GeneratedAudio {
        samples: *const c_float,
        n: i32,
        sample_rate: i32,
    }
    type ProgressCallback = unsafe extern "C" fn(*const c_float, i32, c_float, *mut c_void) -> i32;

    type GetString = unsafe extern "C" fn() -> *const c_char;
    type CreateTts = unsafe extern "C" fn(*const TtsConfig) -> *mut c_void;
    type DestroyTts = unsafe extern "C" fn(*mut c_void);
    type SampleRate = unsafe extern "C" fn(*const c_void) -> i32;
    type NumSpeakers = unsafe extern "C" fn(*const c_void) -> i32;
    type Generate = unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        *const GenerationConfig,
        ProgressCallback,
        *mut c_void,
    ) -> *mut GeneratedAudio;
    type DestroyAudio = unsafe extern "C" fn(*mut GeneratedAudio);

    struct Api {
        _library: Library,
        destroy_tts: DestroyTts,
        generate: Generate,
        destroy_audio: DestroyAudio,
    }

    struct Engine {
        api: Api,
        handle: *mut c_void,
        _paths: Vec<CString>,
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            // SAFETY: `handle` came from this still-loaded API and is destroyed once here.
            unsafe { (self.api.destroy_tts)(self.handle) };
        }
    }

    type SharedOutput = Arc<Mutex<BufWriter<io::Stdout>>>;

    struct Job {
        request_id: String,
        generation: u64,
        text: String,
        speaker_id: i32,
    }

    struct CallbackState {
        output: SharedOutput,
        request_id: String,
        generation: u64,
        current_generation: Arc<AtomicU64>,
        sequence: u64,
        failed: bool,
        previous_input: f32,
        previous_output: f32,
    }

    fn synthesis_segments(text: &str) -> Vec<String> {
        // Kokoro's C callback arrives only after one input fragment finishes.
        // Keep fragments short enough to yield useful first PCM while avoiding
        // tiny, prosodically broken word groups.
        const TARGET_CHARS: usize = 36;
        const MIN_CHARS: usize = 12;
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut last_space = None;
        for character in text.chars() {
            current.push(character);
            let count = current.chars().count();
            if character.is_whitespace() {
                last_space = Some(current.len());
            }
            let natural_boundary = matches!(character, ',' | ';' | ':' | '!' | '?' | '.');
            if natural_boundary && count >= MIN_CHARS {
                segments.push(current.trim().to_owned());
                current.clear();
                last_space = None;
            } else if count >= TARGET_CHARS {
                let split = last_space.unwrap_or(current.len());
                let tail = current.split_off(split);
                let head = current.trim().to_owned();
                if !head.is_empty() {
                    segments.push(head);
                }
                current = tail.trim_start().to_owned();
                last_space = current.rfind(char::is_whitespace).map(|index| index + 1);
            }
        }
        let remainder = current.trim();
        if !remainder.is_empty() {
            segments.push(remainder.to_owned());
        }
        segments
    }

    fn event(
        name: &str,
        request_id: &str,
        sequence: u64,
        end: bool,
        code: Option<&str>,
    ) -> WorkerEvent {
        WorkerEvent {
            event: name.into(),
            request_id: request_id.into(),
            sequence,
            sample_rate_hz: SAMPLE_RATE,
            channels: 1,
            end_of_stream: end,
            code: code.map(str::to_owned),
        }
    }

    fn emit(output: &SharedOutput, header: &WorkerEvent, pcm: &[u8]) -> io::Result<()> {
        let mut output = output
            .lock()
            .map_err(|_| io::Error::other("output lock poisoned"))?;
        write_blocking(&mut *output, header, pcm)
    }

    unsafe extern "C" fn progress(
        samples: *const c_float,
        count: i32,
        _progress: c_float,
        opaque: *mut c_void,
    ) -> i32 {
        let result = std::panic::catch_unwind(|| {
            if opaque.is_null() || samples.is_null() || !(0..=10_000_000).contains(&count) {
                return false;
            }
            // SAFETY: the synchronous sherpa call receives a live CallbackState pointer.
            let state = unsafe { &mut *(opaque.cast::<CallbackState>()) };
            if state.current_generation.load(Ordering::Acquire) > state.generation {
                return false;
            }
            // SAFETY: sherpa owns `count` f32 values for the duration of this callback.
            let values = unsafe { std::slice::from_raw_parts(samples, count as usize) };
            let mut pcm = Vec::with_capacity(values.len() * 2);
            for &sample in values {
                if !sample.is_finite() {
                    state.failed = true;
                    return false;
                }
                // Remove the measurable DC bias in the upstream Kokoro
                // waveform with a continuous one-pole blocker (~19 Hz at
                // 24 kHz). State is preserved across clause fragments.
                let filtered = sample - state.previous_input + 0.995 * state.previous_output;
                state.previous_input = sample;
                state.previous_output = filtered;
                let scaled = (filtered.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
                pcm.extend_from_slice(&scaled.to_le_bytes());
            }
            let header = event("audio", &state.request_id, state.sequence, false, None);
            if emit(&state.output, &header, &pcm).is_err() {
                state.failed = true;
                return false;
            }
            state.sequence = state.sequence.saturating_add(1);
            true
        });
        i32::from(result.unwrap_or(false))
    }

    fn sha256(path: &Path, expected_size: u64) -> io::Result<String> {
        let mut file = File::open(path)?;
        if file.metadata()?.len() != expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "critical file size mismatch",
            ));
        }
        let mut digest = Sha256::new();
        io::copy(&mut file, &mut digest)?;
        Ok(format!("{:x}", digest.finalize()))
    }

    fn verify_pack(root: &Path) -> io::Result<()> {
        if root.file_name().and_then(|value| value.to_str()) != Some(REVISION)
            || root
                .parent()
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                != Some(PACK_ID)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pack identity mismatch",
            ));
        }
        for (relative, size, expected) in CRITICAL {
            let path = root.join(relative);
            if path.symlink_metadata()?.file_type().is_symlink() || sha256(&path, size)? != expected
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "critical file digest mismatch",
                ));
            }
        }
        Ok(())
    }

    unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> io::Result<T> {
        // SAFETY: caller supplies the exact sherpa-onnx 1.13.6 C ABI signature.
        unsafe { library.get::<T>(name) }
            .map(|symbol| *symbol)
            .map_err(io::Error::other)
    }

    fn load_engine(root: &Path, threads: i32) -> io::Result<Engine> {
        verify_pack(root)?;
        let runtime = root.join("runtime/lib");
        let dll = runtime.join("sherpa-onnx-c-api.dll");
        if !dll.is_file() || dll.symlink_metadata()?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "verified runtime is absent",
            ));
        }
        // Force dependencies to resolve beside the selected absolute DLL. This
        // prevents an unrelated onnxruntime.dll in the application/build
        // directory from winning the default Windows loader search order.
        const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
        const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;
        // SAFETY: only the fixed absolute DLL inside the verified pack is
        // loaded, with dependency search restricted to its directory and the
        // standard safe system directories.
        let platform_library = unsafe {
            libloading::os::windows::Library::load_with_flags(
                &dll,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }
        .map_err(io::Error::other)?;
        let library: Library = platform_library.into();
        // SAFETY: each symbol is loaded from the already identity-checked
        // sherpa C API library with the exact pinned 1.13.6 signature.
        let symbols: (
            GetString,
            GetString,
            GetString,
            CreateTts,
            DestroyTts,
            SampleRate,
            NumSpeakers,
            Generate,
            DestroyAudio,
        ) = unsafe {
            (
                symbol(&library, b"SherpaOnnxGetVersionStr\0")?,
                symbol(&library, b"SherpaOnnxGetGitSha1\0")?,
                symbol(&library, b"SherpaOnnxGetOnnxruntimeVersionStr\0")?,
                symbol(&library, b"SherpaOnnxCreateOfflineTts\0")?,
                symbol(&library, b"SherpaOnnxDestroyOfflineTts\0")?,
                symbol(&library, b"SherpaOnnxOfflineTtsSampleRate\0")?,
                symbol(&library, b"SherpaOnnxOfflineTtsNumSpeakers\0")?,
                symbol(&library, b"SherpaOnnxOfflineTtsGenerateWithConfig\0")?,
                symbol(&library, b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0")?,
            )
        };
        let (
            get_version,
            get_sha,
            get_ort,
            create,
            destroy_tts,
            sample_rate,
            speakers,
            generate,
            destroy_audio,
        ) = symbols;
        // SAFETY: the pinned metadata functions return non-null, static,
        // nul-terminated strings for the lifetime of the loaded library.
        let (version, git_sha, ort) = unsafe {
            (
                CStr::from_ptr(get_version()).to_string_lossy(),
                CStr::from_ptr(get_sha()).to_string_lossy(),
                CStr::from_ptr(get_ort()).to_string_lossy(),
            )
        };
        if version.trim_start_matches('v') != "1.13.6"
            || !"1cb484af5e69d3c7803c1eb0b3b5ab8041e0e911".starts_with(git_sha.as_ref())
            || ort != "1.27.1"
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "native runtime identity mismatch: sherpa={version} git={git_sha} ort={ort}"
                ),
            ));
        }
        let required = [
            root.join("model/model.int8.onnx"),
            root.join("model/voices.bin"),
            root.join("model/tokens.txt"),
            root.join("model/espeak-ng-data"),
            root.join("model/lexicon-us-en.txt"),
        ];
        let mut paths = required
            .iter()
            .map(|path| {
                // The pinned phonemizer concatenates its own relative paths and
                // does not understand Rust's `\\?\` extended-length prefix.
                // This pack is far below MAX_PATH, so pass the ordinary drive
                // path after canonical verification.
                let rendered = path.to_string_lossy();
                let rendered = rendered.strip_prefix(r"\\?\").unwrap_or(&rendered);
                CString::new(rendered.as_bytes()).map_err(io::Error::other)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cpu = CString::new("cpu").expect("literal");
        paths.push(cpu);
        // SAFETY: the pinned C ABI treats zero as the documented default for unrelated model families.
        let mut config: TtsConfig = unsafe { std::mem::zeroed() };
        config.model.kokoro.model = paths[0].as_ptr();
        config.model.kokoro.voices = paths[1].as_ptr();
        config.model.kokoro.tokens = paths[2].as_ptr();
        config.model.kokoro.data_dir = paths[3].as_ptr();
        config.model.kokoro.length_scale = 1.0;
        config.model.kokoro.lexicon = paths[4].as_ptr();
        config.model.num_threads = threads;
        config.model.provider = paths[5].as_ptr();
        config.max_num_sentences = 1;
        config.silence_scale = 0.2;
        // SAFETY: every pointer in `config` refers to a live CString retained
        // in `paths`, and all other fields follow the pinned ABI contract.
        let handle = unsafe { create(&config) };
        let metadata_matches = if handle.is_null() {
            false
        } else {
            // SAFETY: a non-null handle returned by `create` remains live here.
            unsafe { sample_rate(handle) == SAMPLE_RATE as i32 && speakers(handle) >= 53 }
        };
        if !metadata_matches {
            if !handle.is_null() {
                // SAFETY: the live handle was created by this same library and
                // has not been transferred or destroyed.
                unsafe { destroy_tts(handle) };
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Kokoro engine metadata mismatch",
            ));
        }
        Ok(Engine {
            api: Api {
                _library: library,
                destroy_tts,
                generate,
                destroy_audio,
            },
            handle,
            _paths: paths,
        })
    }

    fn synthesis_loop(
        engine: Engine,
        jobs: Receiver<Job>,
        output: SharedOutput,
        generation: Arc<AtomicU64>,
        busy: Arc<AtomicBool>,
    ) {
        while let Ok(job) = jobs.recv() {
            let mut state = CallbackState {
                output: Arc::clone(&output),
                request_id: job.request_id.clone(),
                generation: job.generation,
                current_generation: Arc::clone(&generation),
                sequence: 0,
                failed: false,
                previous_input: 0.0,
                previous_output: 0.0,
            };
            let mut native_failed = false;
            for segment in synthesis_segments(&job.text) {
                if generation.load(Ordering::Acquire) > job.generation {
                    break;
                }
                let text = match CString::new(segment) {
                    Ok(value) => value,
                    Err(_) => {
                        native_failed = true;
                        break;
                    }
                };
                // SAFETY: zero initializes all optional generation fields in the pinned ABI.
                let mut config: GenerationConfig = unsafe { std::mem::zeroed() };
                config.silence_scale = 0.2;
                config.speed = 1.0;
                config.sid = job.speaker_id;
                // SAFETY: the engine handle and function pointer belong to the
                // retained library; text/config/state remain live for this
                // synchronous call, and `progress` validates callback inputs.
                let audio = unsafe {
                    (engine.api.generate)(
                        engine.handle,
                        text.as_ptr(),
                        &config,
                        progress,
                        (&mut state as *mut CallbackState).cast(),
                    )
                };
                if audio.is_null() {
                    native_failed = true;
                    break;
                }
                // SAFETY: `audio` is non-null, was returned by this engine's
                // generate call, and has not yet been destroyed.
                unsafe { (engine.api.destroy_audio)(audio) };
            }
            let cancelled = generation.load(Ordering::Acquire) > job.generation;
            let terminal = if cancelled {
                event("cancelled", &job.request_id, state.sequence, true, None)
            } else if state.failed || native_failed || state.sequence == 0 {
                event(
                    "error",
                    &job.request_id,
                    state.sequence,
                    true,
                    Some("native_synthesis_failed"),
                )
            } else {
                event("completed", &job.request_id, state.sequence, true, None)
            };
            let _ = emit(&output, &terminal, &[]);
            busy.store(false, Ordering::Release);
        }
    }

    fn arg(name: &str) -> io::Result<String> {
        let mut args = env::args();
        while let Some(value) = args.next() {
            if value == name {
                return args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "missing argument value")
                });
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("missing {name}"),
        ))
    }

    pub fn run() -> io::Result<()> {
        let root = PathBuf::from(arg("--pack-root")?).canonicalize()?;
        let threads: i32 = arg("--cpu-threads")?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid thread count"))?;
        if !(1..=8).contains(&threads) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "thread count outside range",
            ));
        }
        let output = Arc::new(Mutex::new(BufWriter::new(io::stdout())));
        let generation = Arc::new(AtomicU64::new(0));
        let busy = Arc::new(AtomicBool::new(false));
        let (jobs_tx, jobs_rx): (SyncSender<Job>, Receiver<Job>) = mpsc::sync_channel(1);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let synthesis_output = Arc::clone(&output);
        let synthesis_generation = Arc::clone(&generation);
        let synthesis_busy = Arc::clone(&busy);
        let worker = std::thread::Builder::new()
            .name("kokoro-native-synthesis".into())
            .spawn(move || match load_engine(&root, threads) {
                Ok(engine) => {
                    let _ = ready_tx.send(Ok(()));
                    synthesis_loop(
                        engine,
                        jobs_rx,
                        synthesis_output,
                        synthesis_generation,
                        synthesis_busy,
                    );
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                }
            })?;
        ready_rx
            .recv()
            .map_err(|_| io::Error::other("synthesis thread stopped during load"))?
            .map_err(io::Error::other)?;
        emit(&output, &event("ready", "startup", 0, false, None), &[])?;
        let mut input = BufReader::new(io::stdin());
        while let Some(command) = read_command_blocking(&mut input)? {
            match command {
                WorkerCommand::Synthesize {
                    request_id,
                    generation: request_generation,
                    text,
                    voice_id,
                } => {
                    let voice = VOICES.iter().position(|candidate| *candidate == voice_id);
                    if request_id.is_empty()
                        || text.trim().is_empty()
                        || text.chars().count() > 16_384
                        || voice.is_none()
                        || request_generation < generation.load(Ordering::Acquire)
                        || busy.swap(true, Ordering::AcqRel)
                    {
                        emit(
                            &output,
                            &event(
                                "error",
                                &request_id,
                                0,
                                true,
                                Some("invalid_or_busy_request"),
                            ),
                            &[],
                        )?;
                    } else if jobs_tx
                        .send(Job {
                            request_id,
                            generation: request_generation,
                            text,
                            speaker_id: voice.unwrap_or_default() as i32,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                WorkerCommand::Cancel {
                    request_id,
                    generation: next,
                } => {
                    generation.fetch_max(next, Ordering::AcqRel);
                    emit(
                        &output,
                        &event("cancel_ack", &request_id, 0, true, None),
                        &[],
                    )?;
                }
                WorkerCommand::Shutdown { request_id } => {
                    generation.fetch_add(1, Ordering::AcqRel);
                    emit(
                        &output,
                        &event("shutdown_ack", &request_id, 0, true, None),
                        &[],
                    )?;
                    break;
                }
            }
        }
        drop(jobs_tx);
        worker
            .join()
            .map_err(|_| io::Error::other("synthesis thread panicked"))?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::synthesis_segments;

        #[test]
        fn segments_at_natural_boundaries_and_bounded_spaces() {
            let source = "Before the caravan leaves, the quartermaster checks every crate and records every seal in the weathered ledger.";
            let segments = synthesis_segments(source);
            assert_eq!(segments[0], "Before the caravan leaves,");
            assert!(segments.iter().all(|segment| segment.chars().count() <= 36));
            assert_eq!(segments.concat().replace(' ', ""), source.replace(' ', ""));
        }
    }
}

#[cfg(windows)]
fn main() {
    if windows_worker::run().is_err() {
        std::process::exit(2);
    }
}
