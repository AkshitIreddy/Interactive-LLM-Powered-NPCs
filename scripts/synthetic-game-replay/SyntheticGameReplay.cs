using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace InteractiveNpcs.SyntheticReplay
{
    internal sealed class Options
    {
        public string InputPath;
        public string MetadataPath;
        public string FfmpegPath;
        public string WindowTitle = "Interactive NPCs Synthetic Game - Eclipse Harbor";
        public string PlacementHelper;
        public int Width = 960;
        public int Height = 600;
        public double FramesPerSecond = 15.0;
        public int ExitAfterSeconds;
        public bool Loop = true;
        public bool PlaceOnSecondMonitor;

        public static Options Parse(string[] args)
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            for (var index = 0; index < args.Length; index++)
            {
                var key = args[index];
                if (!key.StartsWith("--", StringComparison.Ordinal))
                {
                    throw new ArgumentException("Unexpected argument: " + key);
                }

                if (string.Equals(key, "--no-loop", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(key, "--place-on-second-monitor", StringComparison.OrdinalIgnoreCase))
                {
                    values[key] = "true";
                    continue;
                }

                if (index + 1 >= args.Length)
                {
                    throw new ArgumentException("Missing value for " + key);
                }

                values[key] = args[++index];
            }

            var options = new Options();
            options.InputPath = Required(values, "--input");
            options.MetadataPath = Required(values, "--metadata");
            options.FfmpegPath = Required(values, "--ffmpeg");
            options.WindowTitle = Value(values, "--title", options.WindowTitle);
            options.PlacementHelper = Value(values, "--placement-helper", null);
            options.Width = Integer(values, "--width", options.Width, 64, 7680);
            options.Height = Integer(values, "--height", options.Height, 64, 4320);
            options.FramesPerSecond = Number(values, "--fps", options.FramesPerSecond, 1, 240);
            options.ExitAfterSeconds = Integer(values, "--exit-after-seconds", 0, 0, 86400);
            options.Loop = !values.ContainsKey("--no-loop");
            options.PlaceOnSecondMonitor = values.ContainsKey("--place-on-second-monitor");
            return options;
        }

        private static string Required(Dictionary<string, string> values, string key)
        {
            string value;
            if (!values.TryGetValue(key, out value) || string.IsNullOrWhiteSpace(value))
            {
                throw new ArgumentException("Required argument missing: " + key);
            }

            return Path.GetFullPath(value);
        }

        private static string Value(Dictionary<string, string> values, string key, string fallback)
        {
            string value;
            return values.TryGetValue(key, out value) ? value : fallback;
        }

        private static int Integer(Dictionary<string, string> values, string key, int fallback, int minimum, int maximum)
        {
            string raw;
            if (!values.TryGetValue(key, out raw))
            {
                return fallback;
            }

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
            if (!values.TryGetValue(key, out raw))
            {
                return fallback;
            }

            double value;
            if (!double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out value) || value < minimum || value > maximum)
            {
                throw new ArgumentException(key + " must be between " + minimum + " and " + maximum + ".");
            }

            return value;
        }
    }

    internal sealed class ReplayForm : Form
    {
        private readonly Options options;
        private readonly object frameGate = new object();
        private readonly System.Windows.Forms.Timer repaintTimer;
        private readonly System.Windows.Forms.Timer exitTimer;
        private readonly CancellationTokenSource cancellation = new CancellationTokenSource();
        private Process decoder;
        private Bitmap currentFrame;
        private string placementStatus = "not-requested";
        private long decodedFrames;
        private int playingMetadataPublished;

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

            repaintTimer = new System.Windows.Forms.Timer();
            repaintTimer.Interval = Math.Max(8, (int)Math.Round(1000.0 / options.FramesPerSecond));
            repaintTimer.Tick += delegate { Invalidate(); };

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
            WriteMetadata("ready", null);
            StartDecoder();
            repaintTimer.Start();
            if (options.ExitAfterSeconds > 0)
            {
                exitTimer.Start();
            }
        }

        protected override void OnPaint(PaintEventArgs eventArgs)
        {
            base.OnPaint(eventArgs);
            Bitmap frame = null;
            lock (frameGate)
            {
                if (currentFrame != null)
                {
                    frame = (Bitmap)currentFrame.Clone();
                }
            }

            if (frame == null)
            {
                using (var brush = new SolidBrush(Color.FromArgb(247, 243, 244)))
                using (var font = new Font("Segoe UI", 16, FontStyle.Regular, GraphicsUnit.Point))
                {
                    eventArgs.Graphics.DrawString("Loading synthetic Eclipse Harbor replay...", font, brush, 28, 28);
                }

                return;
            }

            using (frame)
            {
                eventArgs.Graphics.InterpolationMode = System.Drawing.Drawing2D.InterpolationMode.HighQualityBicubic;
                eventArgs.Graphics.PixelOffsetMode = System.Drawing.Drawing2D.PixelOffsetMode.HighQuality;
                var destination = Fit(frame.Size, ClientSize);
                eventArgs.Graphics.DrawImage(frame, destination);
            }
        }

        private static Rectangle Fit(Size source, Size destination)
        {
            var scale = Math.Min((double)destination.Width / source.Width, (double)destination.Height / source.Height);
            var width = Math.Max(1, (int)Math.Round(source.Width * scale));
            var height = Math.Max(1, (int)Math.Round(source.Height * scale));
            return new Rectangle((destination.Width - width) / 2, (destination.Height - height) / 2, width, height);
        }

        private void StartDecoder()
        {
            var arguments = new StringBuilder();
            arguments.Append("-hide_banner -loglevel error -nostdin -hwaccel none ");
            if (options.Loop)
            {
                arguments.Append("-stream_loop -1 ");
            }

            arguments.Append("-re -i ").Append(Quote(options.InputPath)).Append(' ');
            arguments.Append("-map 0:v:0 -an -sn -dn -threads 2 -filter_threads 1 ");
            arguments.Append("-vf ").Append(Quote("scale=" + options.Width + ":" + options.Height + ":flags=fast_bilinear,fps=" + options.FramesPerSecond.ToString("0.###", CultureInfo.InvariantCulture))).Append(' ');
            arguments.Append("-pix_fmt bgr24 -f rawvideo pipe:1");

            var startInfo = new ProcessStartInfo
            {
                FileName = options.FfmpegPath,
                Arguments = arguments.ToString(),
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                WorkingDirectory = Path.GetDirectoryName(options.InputPath)
            };
            startInfo.EnvironmentVariables["CUDA_VISIBLE_DEVICES"] = "-1";
            startInfo.EnvironmentVariables["NVIDIA_VISIBLE_DEVICES"] = "none";

            decoder = new Process { StartInfo = startInfo, EnableRaisingEvents = true };
            decoder.Exited += delegate
            {
                if (!cancellation.IsCancellationRequested)
                {
                    BeginInvoke(new Action(delegate
                    {
                        WriteMetadata("decoder-exited", "FFmpeg ended before the replay window closed.");
                    }));
                }
            };

            if (!decoder.Start())
            {
                throw new InvalidOperationException("FFmpeg software decoder did not start.");
            }

            Task.Run(() => DrainErrors(decoder.StandardError, cancellation.Token));
            Task.Run(() => ReadFrames(decoder.StandardOutput.BaseStream, cancellation.Token));
        }

        private void ReadFrames(Stream source, CancellationToken token)
        {
            var bytesPerFrame = checked(options.Width * options.Height * 3);
            var buffer = new byte[bytesPerFrame];
            while (!token.IsCancellationRequested)
            {
                var offset = 0;
                while (offset < buffer.Length && !token.IsCancellationRequested)
                {
                    var read = source.Read(buffer, offset, buffer.Length - offset);
                    if (read <= 0)
                    {
                        return;
                    }

                    offset += read;
                }

                if (offset != buffer.Length)
                {
                    return;
                }

                var next = CopyFrame(buffer, options.Width, options.Height);
                lock (frameGate)
                {
                    var previous = currentFrame;
                    currentFrame = next;
                    if (previous != null)
                    {
                        previous.Dispose();
                    }
                }

                Interlocked.Increment(ref decodedFrames);
                if (Interlocked.CompareExchange(ref playingMetadataPublished, 1, 0) == 0)
                {
                    BeginInvoke(new Action(delegate { WriteMetadata("playing", null); }));
                }
            }
        }

        private static Bitmap CopyFrame(byte[] source, int width, int height)
        {
            var bitmap = new Bitmap(width, height, PixelFormat.Format24bppRgb);
            var bounds = new Rectangle(0, 0, width, height);
            var data = bitmap.LockBits(bounds, ImageLockMode.WriteOnly, PixelFormat.Format24bppRgb);
            try
            {
                var sourceStride = width * 3;
                for (var row = 0; row < height; row++)
                {
                    Marshal.Copy(source, row * sourceStride, IntPtr.Add(data.Scan0, row * data.Stride), sourceStride);
                }
            }
            finally
            {
                bitmap.UnlockBits(data);
            }

            return bitmap;
        }

        private static void DrainErrors(StreamReader reader, CancellationToken token)
        {
            var buffer = new char[1024];
            while (!token.IsCancellationRequested)
            {
                if (reader.Read(buffer, 0, buffer.Length) <= 0)
                {
                    return;
                }
            }
        }

        private string PlaceWindowIfRequested()
        {
            if (!options.PlaceOnSecondMonitor)
            {
                return "not-requested";
            }

            if (string.IsNullOrWhiteSpace(options.PlacementHelper) || !File.Exists(options.PlacementHelper))
            {
                return "helper-unavailable";
            }

            try
            {
                var powerShell = Path.Combine(Environment.SystemDirectory, "WindowsPowerShell", "v1.0", "powershell.exe");
                var dryRun = RunPlacement(powerShell, false);
                if (dryRun != 0)
                {
                    return "dry-run-failed:" + dryRun;
                }

                var apply = RunPlacement(powerShell, true);
                return apply == 0 ? "applied-or-primary-fallback" : "apply-failed:" + apply;
            }
            catch (Exception exception)
            {
                return "placement-error:" + exception.GetType().Name;
            }
        }

        private int RunPlacement(string powerShell, bool apply)
        {
            var arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File " + Quote(options.PlacementHelper) +
                " -TargetProcessId " + Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture) + (apply ? " -Apply" : string.Empty);
            var process = Process.Start(new ProcessStartInfo
            {
                FileName = powerShell,
                Arguments = arguments,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            });
            if (!process.WaitForExit(10000))
            {
                process.Kill();
                return 124;
            }

            return process.ExitCode;
        }

        private void WriteMetadata(string state, string error)
        {
            var process = Process.GetCurrentProcess();
            var payload = new Dictionary<string, object>
            {
                { "schema_version", 1 },
                { "fixture_kind", "synthetic-original-video-replay" },
                { "state", state },
                { "generated_at_utc", DateTime.UtcNow.ToString("o", CultureInfo.InvariantCulture) },
                { "pid", process.Id },
                { "process_id", process.Id },
                { "window_handle", Handle.ToInt64() },
                { "hwnd", Handle.ToInt64() },
                { "hwnd_hex", "0x" + Handle.ToInt64().ToString("X", CultureInfo.InvariantCulture) },
                { "executable_basename", Path.GetFileName(process.MainModule.FileName) },
                { "exe_basename", Path.GetFileName(process.MainModule.FileName) },
                { "window_title", Text },
                { "input_path", options.InputPath },
                { "width", options.Width },
                { "height", options.Height },
                { "frames_per_second", options.FramesPerSecond },
                { "loop", options.Loop },
                { "decoded_frames", Interlocked.Read(ref decodedFrames) },
                { "decoder", "ffmpeg-software-bgr24" },
                { "hardware_acceleration", false },
                { "nvidia_compute_requested", false },
                { "placement_status", placementStatus },
                { "error", error }
            };

            var directory = Path.GetDirectoryName(options.MetadataPath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var temporaryPath = options.MetadataPath + "." + process.Id.ToString(CultureInfo.InvariantCulture) + ".tmp";
            File.WriteAllText(temporaryPath, new JavaScriptSerializer().Serialize(payload), new UTF8Encoding(false));
            if (File.Exists(options.MetadataPath))
            {
                File.Replace(temporaryPath, options.MetadataPath, null, true);
            }
            else
            {
                File.Move(temporaryPath, options.MetadataPath);
            }
        }

        private void OnFormClosing(object sender, FormClosingEventArgs eventArgs)
        {
            cancellation.Cancel();
            repaintTimer.Stop();
            exitTimer.Stop();
            if (decoder != null && !decoder.HasExited)
            {
                try
                {
                    decoder.Kill();
                    decoder.WaitForExit(3000);
                }
                catch
                {
                    // Process teardown is best-effort during window close.
                }
            }

            lock (frameGate)
            {
                if (currentFrame != null)
                {
                    currentFrame.Dispose();
                    currentFrame = null;
                }
            }

            WriteMetadata("closed", null);
        }

        private static string Quote(string value)
        {
            return "\"" + value.Replace("\"", "\\\"") + "\"";
        }
    }

    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            try
            {
                var options = Options.Parse(args);
                if (!File.Exists(options.InputPath))
                {
                    throw new FileNotFoundException("Synthetic replay MP4 was not found.", options.InputPath);
                }

                if (!File.Exists(options.FfmpegPath))
                {
                    throw new FileNotFoundException("FFmpeg was not found.", options.FfmpegPath);
                }

                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new ReplayForm(options));
                return 0;
            }
            catch (Exception exception)
            {
                Debug.WriteLine("Synthetic replay failed: " + exception);
                return 1;
            }
        }
    }
}
