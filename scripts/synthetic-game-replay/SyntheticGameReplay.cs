// SPDX-License-Identifier: MIT
// Project-owned deterministic Windows capture target. No third-party media or binaries.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Globalization;
using System.IO;
using System.Media;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace InteractiveNpcs.SyntheticReplay
{
    internal sealed class Options
    {
        public string MetadataPath;
        public string WindowTitle = "Interactive NPCs Synthetic Game - Eclipse Harbor";
        public string PlacementHelper;
        public string SelfTestReportPath;
        public string SelfTestFrameDirectory;
        public int Width = 960;
        public int Height = 600;
        public double FramesPerSecond = 15.0;
        public int ExitAfterSeconds;
        public bool Loop = true;
        public bool PlaceOnSecondMonitor;
        public bool Muted;

        public static Options Parse(string[] args)
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var allowed = new HashSet<string>(StringComparer.OrdinalIgnoreCase) {
                "--metadata", "--title", "--placement-helper", "--self-test-report",
                "--self-test-frame-directory", "--width", "--height", "--fps",
                "--exit-after-seconds", "--no-loop", "--place-on-second-monitor", "--mute"
            };
            for (var index = 0; index < args.Length; index++)
            {
                var key = args[index];
                if (!key.StartsWith("--", StringComparison.Ordinal)) throw new ArgumentException("Unexpected argument: " + key);
                if (!allowed.Contains(key)) throw new ArgumentException("Unknown argument: " + key);
                if (string.Equals(key, "--no-loop", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(key, "--place-on-second-monitor", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(key, "--mute", StringComparison.OrdinalIgnoreCase))
                {
                    values[key] = "true";
                    continue;
                }
                if (index + 1 >= args.Length) throw new ArgumentException("Missing value for " + key);
                values[key] = args[++index];
            }

            var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
            var options = new Options();
            options.MetadataPath = FullPath(values, "--metadata", Path.Combine(appData, "io.github.akshitireddy.interactive-npcs", "debug-synthetic-replay-target.json"));
            options.WindowTitle = Value(values, "--title", options.WindowTitle);
            options.PlacementHelper = Value(values, "--placement-helper", null);
            options.SelfTestReportPath = OptionalFullPath(values, "--self-test-report");
            options.SelfTestFrameDirectory = OptionalFullPath(values, "--self-test-frame-directory");
            options.Width = Integer(values, "--width", options.Width, 320, 7680);
            options.Height = Integer(values, "--height", options.Height, 240, 4320);
            options.FramesPerSecond = Number(values, "--fps", options.FramesPerSecond, 1, 240);
            options.ExitAfterSeconds = Integer(values, "--exit-after-seconds", 0, 0, 86400);
            options.Loop = !values.ContainsKey("--no-loop");
            options.PlaceOnSecondMonitor = values.ContainsKey("--place-on-second-monitor");
            options.Muted = values.ContainsKey("--mute");
            return options;
        }

        private static string Value(Dictionary<string, string> values, string key, string fallback)
        {
            string value;
            return values.TryGetValue(key, out value) ? value : fallback;
        }

        private static string FullPath(Dictionary<string, string> values, string key, string fallback)
        {
            return Path.GetFullPath(Value(values, key, fallback));
        }

        private static string OptionalFullPath(Dictionary<string, string> values, string key)
        {
            string value;
            return values.TryGetValue(key, out value) ? Path.GetFullPath(value) : null;
        }

        private static int Integer(Dictionary<string, string> values, string key, int fallback, int minimum, int maximum)
        {
            string raw;
            if (!values.TryGetValue(key, out raw)) return fallback;
            int value;
            if (!int.TryParse(raw, NumberStyles.Integer, CultureInfo.InvariantCulture, out value) || value < minimum || value > maximum)
            {
                throw new ArgumentException(key + " must be between " + minimum + " and " + maximum + ".");
            }
            return value;
        }

        private static double Number(Dictionary<string, string> values, string key, double fallback, double minimum, double maximum)
        {
            string raw;
            if (!values.TryGetValue(key, out raw)) return fallback;
            double value;
            if (!double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out value) || value < minimum || value > maximum)
            {
                throw new ArgumentException(key + " must be between " + minimum + " and " + maximum + ".");
            }
            return value;
        }
    }

    internal sealed class AudioMetrics
    {
        public int DurationSeconds;
        public int SampleRate;
        public int SampleCount;
        public int Peak;
        public double Rms;
        public int NonzeroSamples;
        public int ClippedSamples;
        public int LoopBoundaryDelta;
    }

    internal static class SyntheticScene
    {
        public const int LoopSeconds = 12;
        public const int AudioSampleRate = 22050;
        public const string PortraitResourceName = "InteractiveNpcs.SyntheticReplay.MaraVennPortraitV1.png";
        public const string PortraitSha256 = "0ae10605199bb831c34a3be29c31a06f8a1d5a7e5cd07b8a1456866f2bb7dc2d";
        public const string VisualSource = "embedded-original-generated-photorealistic-portrait-v1";
        private static readonly byte[] PortraitBytes = LoadPortraitBytes();
        private static readonly Image Portrait = LoadPortrait();

        public static Bitmap RenderFrame(int width, int height, double seconds)
        {
            var bitmap = new Bitmap(width, height);
            using (var graphics = Graphics.FromImage(bitmap))
            {
                graphics.SmoothingMode = SmoothingMode.AntiAlias;
                graphics.TextRenderingHint = System.Drawing.Text.TextRenderingHint.ClearTypeGridFit;
                graphics.ScaleTransform(width / 960.0f, height / 600.0f);

                var phase = (seconds % LoopSeconds) / LoopSeconds;
                var beacon = 0.5f + 0.5f * (float)Math.Sin(phase * Math.PI * 8.0);
                DrawCover(graphics, Portrait, new Rectangle(0, 0, 960, 600));

                // Motion is deliberately confined to the periphery. The portrait and its mouth
                // remain pixel-identical between source frames, so fixture motion cannot be
                // mistaken for a successful product lipsync overlay.
                using (var rain = new Pen(Color.FromArgb(72, 190, 220, 232), 1.25f))
                {
                    for (var index = 0; index < 20; index++)
                    {
                        var x = index < 10 ? 18 + index * 31 : 684 + (index - 10) * 27;
                        var y = (float)((index * 61 + seconds * 125.0) % 620.0) - 20.0f;
                        graphics.DrawLine(rain, x, y, x - 7, y + 22);
                    }
                }

                using (var panel = new SolidBrush(Color.FromArgb(205, 7, 10, 24)))
                using (var border = new Pen(Color.FromArgb(170, 255, 176, 124), 1.5f))
                {
                    graphics.FillRectangle(panel, 24, 22, 318, 88);
                    graphics.DrawRectangle(border, 24, 22, 318, 88);
                }
                using (var title = new Font("Segoe UI Semibold", 19, FontStyle.Bold, GraphicsUnit.Point))
                using (var small = new Font("Consolas", 9, FontStyle.Regular, GraphicsUnit.Point))
                using (var titleBrush = new SolidBrush(Color.FromArgb(255, 244, 225)))
                using (var mutedBrush = new SolidBrush(Color.FromArgb(205, 210, 202, 208)))
                {
                    graphics.DrawString("ECLIPSE HARBOR", title, titleBrush, 40, 34);
                    graphics.DrawString("REALISTIC CAPTURE TARGET  //  MOUTH-STATIC SOURCE", small, mutedBrush, 42, 78);
                }
                using (var beaconBrush = new SolidBrush(Color.FromArgb((int)(80 + beacon * 175), 255, 94, 115))) graphics.FillEllipse(beaconBrush, 905, 28, 20, 20);
                using (var hudFont = new Font("Consolas", 9, FontStyle.Regular, GraphicsUnit.Point))
                using (var hudBrush = new SolidBrush(Color.FromArgb(230, 235, 238, 242)))
                {
                    graphics.DrawString("EMBEDDED PORTRAIT  /  GENERATED PCM", hudFont, hudBrush, 678, 59);
                }
            }
            return bitmap;
        }

        public static Rectangle MouthGuardRectangle(int width, int height)
        {
            return new Rectangle(
                (int)Math.Round(width * 0.455),
                (int)Math.Round(height * 0.455),
                Math.Max(1, (int)Math.Round(width * 0.115)),
                Math.Max(1, (int)Math.Round(height * 0.105)));
        }

        public static string EmbeddedPortraitHash() { return HashBytes(PortraitBytes); }

        private static void DrawCover(Graphics graphics, Image image, Rectangle destination)
        {
            var scale = Math.Max((double)destination.Width / image.Width, (double)destination.Height / image.Height);
            var sourceWidth = destination.Width / scale;
            var sourceHeight = destination.Height / scale;
            var source = new RectangleF(
                (float)((image.Width - sourceWidth) / 2.0),
                (float)((image.Height - sourceHeight) / 2.0),
                (float)sourceWidth,
                (float)sourceHeight);
            graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            graphics.DrawImage(image, destination, source, GraphicsUnit.Pixel);
        }

        private static byte[] LoadPortraitBytes()
        {
            using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(PortraitResourceName))
            {
                if (stream == null) throw new InvalidOperationException("Embedded realistic portrait is missing: " + PortraitResourceName);
                using (var buffer = new MemoryStream()) { stream.CopyTo(buffer); return buffer.ToArray(); }
            }
        }

        private static Image LoadPortrait()
        {
            var actualHash = HashBytes(PortraitBytes);
            if (!string.Equals(actualHash, PortraitSha256, StringComparison.OrdinalIgnoreCase))
            {
                throw new InvalidOperationException("Embedded portrait hash mismatch.");
            }
            using (var stream = new MemoryStream(PortraitBytes, false))
            using (var decoded = Image.FromStream(stream, true, true))
            {
                return new Bitmap(decoded);
            }
        }

        private static string HashBytes(byte[] bytes)
        {
            using (var hash = SHA256.Create())
            {
                var digest = hash.ComputeHash(bytes);
                var builder = new StringBuilder(digest.Length * 2);
                foreach (var value in digest) builder.Append(value.ToString("x2", CultureInfo.InvariantCulture));
                return builder.ToString();
            }
        }

        public static byte[] CreateWave(out AudioMetrics metrics)
        {
            var sampleCount = AudioSampleRate * LoopSeconds;
            var pcm = new short[sampleCount];
            double sumSquares = 0;
            var peak = 0;
            var nonzero = 0;
            for (var index = 0; index < sampleCount; index++)
            {
                var seconds = (double)index / AudioSampleRate;
                var envelope = 0.72 + 0.08 * Math.Sin(2.0 * Math.PI * seconds / 3.0);
                var sample = envelope * (Math.Sin(2.0 * Math.PI * 110.0 * seconds) * 0.030 + Math.Sin(2.0 * Math.PI * 165.0 * seconds) * 0.018 + Math.Sin(2.0 * Math.PI * 220.0 * seconds) * 0.009);
                var value = (short)Math.Round(sample * short.MaxValue);
                pcm[index] = value;
                var magnitude = Math.Abs((int)value);
                if (magnitude > peak) peak = magnitude;
                if (value != 0) nonzero++;
                sumSquares += value * (double)value;
            }
            metrics = new AudioMetrics {
                DurationSeconds = LoopSeconds, SampleRate = AudioSampleRate, SampleCount = sampleCount,
                Peak = peak, Rms = Math.Sqrt(sumSquares / sampleCount), NonzeroSamples = nonzero,
                ClippedSamples = 0, LoopBoundaryDelta = Math.Abs((int)pcm[0] - (int)pcm[pcm.Length - 1])
            };
            using (var stream = new MemoryStream())
            using (var writer = new BinaryWriter(stream, Encoding.ASCII))
            {
                writer.Write(Encoding.ASCII.GetBytes("RIFF"));
                writer.Write(36 + sampleCount * 2);
                writer.Write(Encoding.ASCII.GetBytes("WAVEfmt "));
                writer.Write(16);
                writer.Write((short)1);
                writer.Write((short)1);
                writer.Write(AudioSampleRate);
                writer.Write(AudioSampleRate * 2);
                writer.Write((short)2);
                writer.Write((short)16);
                writer.Write(Encoding.ASCII.GetBytes("data"));
                writer.Write(sampleCount * 2);
                for (var index = 0; index < pcm.Length; index++) writer.Write(pcm[index]);
                writer.Flush();
                return stream.ToArray();
            }
        }
    }

    internal sealed class ReplayForm : Form
    {
        private readonly Options options;
        private readonly object frameGate = new object();
        private readonly System.Windows.Forms.Timer frameTimer;
        private readonly System.Windows.Forms.Timer exitTimer;
        private Bitmap currentFrame;
        private MemoryStream audioStream;
        private SoundPlayer audioPlayer;
        private string placementStatus = "not-requested";
        private long generatedFrames;
        private bool audioStarted;

        public ReplayForm(Options options)
        {
            this.options = options;
            Text = options.WindowTitle;
            Name = "InteractiveNpcsSyntheticGameWindow";
            BackColor = Color.FromArgb(9, 3, 8);
            ClientSize = new Size(options.Width, options.Height);
            MinimumSize = new Size(640, 400);
            StartPosition = FormStartPosition.CenterScreen;
            FormBorderStyle = FormBorderStyle.Sizable;
            MaximizeBox = true;
            DoubleBuffered = true;
            SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.UserPaint, true);
            frameTimer = new System.Windows.Forms.Timer();
            frameTimer.Interval = Math.Max(8, (int)Math.Round(1000.0 / options.FramesPerSecond));
            frameTimer.Tick += delegate { AdvanceFrame(); };
            exitTimer = new System.Windows.Forms.Timer();
            if (options.ExitAfterSeconds > 0)
            {
                exitTimer.Interval = checked(options.ExitAfterSeconds * 1000);
                exitTimer.Tick += delegate { Close(); };
            }
            Shown += OnShown;
            FormClosing += OnFormClosing;
        }

        private void OnShown(object sender, EventArgs eventArgs)
        {
            placementStatus = PlaceWindowIfRequested();
            GenerateFrame(0);
            StartAudio();
            WriteMetadata("playing", null);
            frameTimer.Start();
            if (options.ExitAfterSeconds > 0) exitTimer.Start();
        }

        private void AdvanceFrame()
        {
            var frameIndex = Interlocked.Read(ref generatedFrames);
            if (!options.Loop && frameIndex >= (long)Math.Round(SyntheticScene.LoopSeconds * options.FramesPerSecond))
            {
                Close();
                return;
            }
            GenerateFrame(frameIndex);
            if (frameIndex % Math.Max(1, (int)Math.Round(options.FramesPerSecond)) == 0) WriteMetadata("playing", null);
        }

        private void GenerateFrame(long frameIndex)
        {
            var next = SyntheticScene.RenderFrame(options.Width, options.Height, frameIndex / options.FramesPerSecond);
            lock (frameGate)
            {
                var previous = currentFrame;
                currentFrame = next;
                if (previous != null) previous.Dispose();
            }
            Interlocked.Increment(ref generatedFrames);
            Invalidate();
        }

        private void StartAudio()
        {
            if (options.Muted) return;
            AudioMetrics ignored;
            audioStream = new MemoryStream(SyntheticScene.CreateWave(out ignored), false);
            audioPlayer = new SoundPlayer(audioStream);
            audioPlayer.Load();
            audioPlayer.PlayLooping();
            audioStarted = true;
        }

        protected override void OnPaint(PaintEventArgs eventArgs)
        {
            base.OnPaint(eventArgs);
            Bitmap frame = null;
            lock (frameGate) if (currentFrame != null) frame = (Bitmap)currentFrame.Clone();
            if (frame == null) return;
            using (frame)
            {
                eventArgs.Graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                eventArgs.Graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                eventArgs.Graphics.DrawImage(frame, Fit(frame.Size, ClientSize));
            }
        }

        private static Rectangle Fit(Size source, Size destination)
        {
            var scale = Math.Min((double)destination.Width / source.Width, (double)destination.Height / source.Height);
            var width = Math.Max(1, (int)Math.Round(source.Width * scale));
            var height = Math.Max(1, (int)Math.Round(source.Height * scale));
            return new Rectangle((destination.Width - width) / 2, (destination.Height - height) / 2, width, height);
        }

        private string PlaceWindowIfRequested()
        {
            if (!options.PlaceOnSecondMonitor) return "not-requested";
            if (string.IsNullOrWhiteSpace(options.PlacementHelper) || !File.Exists(options.PlacementHelper)) return "helper-unavailable";
            try
            {
                var powerShell = Path.Combine(Environment.SystemDirectory, "WindowsPowerShell", "v1.0", "powershell.exe");
                var dryRun = RunPlacement(powerShell, false);
                if (dryRun != 0) return "dry-run-failed:" + dryRun;
                var apply = RunPlacement(powerShell, true);
                return apply == 0 ? "applied-or-primary-fallback" : "apply-failed:" + apply;
            }
            catch (Exception exception) { return "placement-error:" + exception.GetType().Name; }
        }

        private int RunPlacement(string powerShell, bool apply)
        {
            var arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File " + Quote(options.PlacementHelper) + " -TargetProcessId " + Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture) + (apply ? " -Apply" : string.Empty);
            var process = Process.Start(new ProcessStartInfo { FileName = powerShell, Arguments = arguments, UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true });
            if (!process.WaitForExit(10000)) { process.Kill(); return 124; }
            return process.ExitCode;
        }

        private void WriteMetadata(string state, string error)
        {
            var process = Process.GetCurrentProcess();
            var payload = new Dictionary<string, object> {
                { "schema_version", 1 },
                { "fixture_kind", "synthetic-original-video-replay" },
                { "fixture_source", "project-source-generated-native-v1" },
                { "state", state }, { "generated_at_utc", DateTime.UtcNow.ToString("o", CultureInfo.InvariantCulture) },
                { "pid", process.Id }, { "process_id", process.Id }, { "window_handle", Handle.ToInt64() }, { "hwnd", Handle.ToInt64() },
                { "hwnd_hex", "0x" + Handle.ToInt64().ToString("X", CultureInfo.InvariantCulture) },
                { "executable_basename", Path.GetFileName(process.MainModule.FileName) }, { "exe_basename", Path.GetFileName(process.MainModule.FileName) },
                { "window_title", Text }, { "width", options.Width }, { "height", options.Height }, { "frames_per_second", options.FramesPerSecond },
                { "loop", options.Loop }, { "generated_frames", Interlocked.Read(ref generatedFrames) }, { "decoded_frames", Interlocked.Read(ref generatedFrames) },
                { "renderer", "project-owned-gdi-generated-v1" }, { "decoder", "none-generated-frames" },
                { "visual_source", SyntheticScene.VisualSource }, { "portrait_sha256", SyntheticScene.PortraitSha256 },
                { "source_mouth_motion", false },
                { "audio_source", "project-owned-generated-pcm-v1" }, { "audio_output", options.Muted ? "muted" : "windows-soundplayer-waveout" },
                { "audio_output_started", audioStarted }, { "hardware_acceleration", false }, { "nvidia_compute_requested", false },
                { "third_party_binaries_loaded", false }, { "placement_status", placementStatus }, { "error", error }
            };
            var directory = Path.GetDirectoryName(options.MetadataPath);
            if (!string.IsNullOrEmpty(directory)) Directory.CreateDirectory(directory);
            var temporaryPath = options.MetadataPath + "." + process.Id.ToString(CultureInfo.InvariantCulture) + ".tmp";
            File.WriteAllText(temporaryPath, new JavaScriptSerializer().Serialize(payload), new UTF8Encoding(false));
            if (File.Exists(options.MetadataPath)) File.Replace(temporaryPath, options.MetadataPath, null, true); else File.Move(temporaryPath, options.MetadataPath);
        }

        private void OnFormClosing(object sender, FormClosingEventArgs eventArgs)
        {
            frameTimer.Stop();
            exitTimer.Stop();
            if (audioPlayer != null) { audioPlayer.Stop(); audioPlayer.Dispose(); }
            if (audioStream != null) audioStream.Dispose();
            lock (frameGate) if (currentFrame != null) { currentFrame.Dispose(); currentFrame = null; }
            WriteMetadata("closed", null);
        }

        private static string Quote(string value) { return "\"" + value.Replace("\"", "\\\"") + "\""; }
    }

    internal static class SelfTest
    {
        public static int Run(Options options)
        {
            var frameDirectory = options.SelfTestFrameDirectory ?? Path.Combine(Path.GetDirectoryName(options.SelfTestReportPath), "frames");
            Directory.CreateDirectory(frameDirectory);
            var frameAPath = Path.Combine(frameDirectory, "synthetic-frame-000.png");
            var frameBPath = Path.Combine(frameDirectory, "synthetic-frame-045.png");
            string frameAHash;
            string frameBHash;
            string mouthAHash;
            string mouthBHash;
            var mouthGuard = SyntheticScene.MouthGuardRectangle(options.Width, options.Height);
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, 0)) { frame.Save(frameAPath, System.Drawing.Imaging.ImageFormat.Png); frameAHash = HashFile(frameAPath); mouthAHash = HashRegion(frame, mouthGuard); }
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, 3)) { frame.Save(frameBPath, System.Drawing.Imaging.ImageFormat.Png); frameBHash = HashFile(frameBPath); mouthBHash = HashRegion(frame, mouthGuard); }
            AudioMetrics audio;
            var waveHash = HashBytes(SyntheticScene.CreateWave(out audio));
            var framesDiffer = !string.Equals(frameAHash, frameBHash, StringComparison.OrdinalIgnoreCase);
            var mouthRegionInvariant = string.Equals(mouthAHash, mouthBHash, StringComparison.OrdinalIgnoreCase);
            var audioPassed = audio.NonzeroSamples > audio.SampleCount / 2 && audio.Peak > 256 && audio.Peak < short.MaxValue && audio.Rms > 128 && audio.ClippedSamples == 0 && audio.LoopBoundaryDelta < 256;
            var portraitHash = SyntheticScene.EmbeddedPortraitHash();
            var portraitVerified = string.Equals(portraitHash, SyntheticScene.PortraitSha256, StringComparison.OrdinalIgnoreCase);
            var passed = framesDiffer && mouthRegionInvariant && portraitVerified && audioPassed;
            var payload = new Dictionary<string, object> {
                { "schema_version", 1 }, { "status", passed ? "passed" : "failed" }, { "fixture_source", "project-source-generated-native-v1" },
                { "renderer", "project-owned-gdi-generated-v1" }, { "generated_frame_count", 2 }, { "frames_differ", framesDiffer },
                { "visual_source", SyntheticScene.VisualSource }, { "portrait_sha256", portraitHash }, { "portrait_hash_verified", portraitVerified },
                { "source_mouth_motion", false }, { "mouth_guard_x", mouthGuard.X }, { "mouth_guard_y", mouthGuard.Y },
                { "mouth_guard_width", mouthGuard.Width }, { "mouth_guard_height", mouthGuard.Height },
                { "mouth_region_frame_a_sha256", mouthAHash }, { "mouth_region_frame_b_sha256", mouthBHash }, { "mouth_region_invariant", mouthRegionInvariant },
                { "frame_a_path", frameAPath }, { "frame_a_sha256", frameAHash }, { "frame_b_path", frameBPath }, { "frame_b_sha256", frameBHash },
                { "audio_source", "project-owned-generated-pcm-v1" }, { "audio_sha256", waveHash }, { "audio_duration_seconds", audio.DurationSeconds },
                { "audio_sample_rate", audio.SampleRate }, { "audio_sample_count", audio.SampleCount }, { "audio_nonzero_samples", audio.NonzeroSamples },
                { "audio_peak", audio.Peak }, { "audio_rms", audio.Rms }, { "audio_clipped_samples", audio.ClippedSamples },
                { "audio_loop_boundary_delta", audio.LoopBoundaryDelta }, { "hardware_acceleration", false }, { "nvidia_compute_requested", false },
                { "third_party_binaries_loaded", false }
            };
            WriteJson(options.SelfTestReportPath, payload);
            return passed ? 0 : 2;
        }

        private static void WriteJson(string path, Dictionary<string, object> payload)
        {
            var directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(directory)) Directory.CreateDirectory(directory);
            File.WriteAllText(path, new JavaScriptSerializer().Serialize(payload), new UTF8Encoding(false));
        }
        private static string HashFile(string path) { using (var stream = File.OpenRead(path)) using (var hash = SHA256.Create()) return Hex(hash.ComputeHash(stream)); }
        private static string HashBytes(byte[] bytes) { using (var hash = SHA256.Create()) return Hex(hash.ComputeHash(bytes)); }
        private static string HashRegion(Bitmap bitmap, Rectangle region)
        {
            using (var buffer = new MemoryStream())
            using (var writer = new BinaryWriter(buffer))
            {
                for (var y = region.Top; y < region.Bottom; y++)
                for (var x = region.Left; x < region.Right; x++) writer.Write(bitmap.GetPixel(x, y).ToArgb());
                writer.Flush();
                return HashBytes(buffer.ToArray());
            }
        }
        private static string Hex(byte[] bytes) { var builder = new StringBuilder(bytes.Length * 2); foreach (var value in bytes) builder.Append(value.ToString("x2", CultureInfo.InvariantCulture)); return builder.ToString(); }
    }

    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            try
            {
                var options = Options.Parse(args);
                if (!string.IsNullOrWhiteSpace(options.SelfTestReportPath)) return SelfTest.Run(options);
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new ReplayForm(options));
                return 0;
            }
            catch (Exception exception) { Debug.WriteLine("Synthetic replay failed: " + exception); return 1; }
        }
    }
}
