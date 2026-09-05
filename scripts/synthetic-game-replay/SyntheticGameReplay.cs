// SPDX-License-Identifier: MIT
// Project-owned deterministic Windows capture target. No third-party media or binaries.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Media;
using System.Reflection;
using System.Runtime.InteropServices;
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
        public string HeadlessExportDirectory;
        public string SourceMouthLandmarksPath;
        public string PlaybackMode = "auto";
        public string SequencePath;
        public int Width = 960;
        public int Height = 600;
        public double FramesPerSecond = 15.0;
        public int ExitAfterSeconds;
        public int HeadlessExportFrameCount = 36;
        public double HeadlessExportStartSeconds;
        public bool Loop = true;
        public bool PlaceOnSecondMonitor;
        public bool Muted = true;

        public static Options Parse(string[] args)
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var allowed = new HashSet<string>(StringComparer.OrdinalIgnoreCase) {
                "--metadata", "--title", "--placement-helper", "--self-test-report",
                "--self-test-frame-directory", "--headless-export-directory", "--source-mouth-landmarks",
                "--headless-export-frame-count", "--headless-export-start-seconds",
                "--mode", "--sequence", "--width", "--height", "--fps",
                "--exit-after-seconds", "--no-loop", "--place-on-second-monitor", "--mute", "--audio"
            };
            for (var index = 0; index < args.Length; index++)
            {
                var key = args[index];
                if (!key.StartsWith("--", StringComparison.Ordinal)) throw new ArgumentException("Unexpected argument: " + key);
                if (!allowed.Contains(key)) throw new ArgumentException("Unknown argument: " + key);
                if (string.Equals(key, "--no-loop", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(key, "--place-on-second-monitor", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(key, "--mute", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(key, "--audio", StringComparison.OrdinalIgnoreCase))
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
            options.HeadlessExportDirectory = OptionalFullPath(values, "--headless-export-directory");
            options.SourceMouthLandmarksPath = OptionalFullPath(values, "--source-mouth-landmarks");
            options.PlaybackMode = Value(values, "--mode", options.PlaybackMode).ToLowerInvariant();
            if (options.PlaybackMode != "auto" && options.PlaybackMode != "moving" && options.PlaybackMode != "static")
                throw new ArgumentException("--mode must be auto, moving, or static.");
            options.SequencePath = OptionalFullPath(values, "--sequence");
            options.Width = Integer(values, "--width", options.Width, 320, 7680);
            options.Height = Integer(values, "--height", options.Height, 240, 4320);
            options.FramesPerSecond = Number(values, "--fps", options.FramesPerSecond, 1, 240);
            options.ExitAfterSeconds = Integer(values, "--exit-after-seconds", 0, 0, 86400);
            options.HeadlessExportFrameCount = Integer(values, "--headless-export-frame-count", options.HeadlessExportFrameCount, 2, 900);
            options.HeadlessExportStartSeconds = Number(values, "--headless-export-start-seconds", 0, 0, 86400);
            options.Loop = !values.ContainsKey("--no-loop");
            options.PlaceOnSecondMonitor = values.ContainsKey("--place-on-second-monitor");
            if (values.ContainsKey("--audio") && values.ContainsKey("--mute")) throw new ArgumentException("--audio and --mute cannot be used together.");
            options.Muted = !values.ContainsKey("--audio");
            if (!string.IsNullOrWhiteSpace(options.SelfTestReportPath) && !string.IsNullOrWhiteSpace(options.HeadlessExportDirectory))
                throw new ArgumentException("--self-test-report and --headless-export-directory are mutually exclusive.");
            if (!string.IsNullOrWhiteSpace(options.HeadlessExportDirectory) && options.PlaybackMode == "static")
                throw new ArgumentException("Headless sequence export requires moving mode.");
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
                throw new ArgumentException(key + " must be between " + minimum + " and " + maximum + ".");
            return value;
        }

        private static double Number(Dictionary<string, string> values, string key, double fallback, double minimum, double maximum)
        {
            string raw;
            if (!values.TryGetValue(key, out raw)) return fallback;
            double value;
            if (!double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out value) || value < minimum || value > maximum)
                throw new ArgumentException(key + " must be between " + minimum + " and " + maximum + ".");
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

    internal sealed class FrameSequence
    {
        private readonly byte[] rgbFrames;
        private readonly byte[] bgrScratch;

        public readonly int FrameCount;
        public readonly int Width;
        public readonly int Height;
        public readonly int FramesPerSecond;
        public readonly string SequenceSha256;
        public readonly string RawPixelsSha256;

        private FrameSequence(byte[] rgbFrames, int frameCount, int width, int height, int framesPerSecond, string sequenceSha256, string rawPixelsSha256)
        {
            this.rgbFrames = rgbFrames;
            bgrScratch = new byte[checked(width * height * 3)];
            FrameCount = frameCount;
            Width = width;
            Height = height;
            FramesPerSecond = framesPerSecond;
            SequenceSha256 = sequenceSha256;
            RawPixelsSha256 = rawPixelsSha256;
        }

        public static FrameSequence Load(string path)
        {
            if (string.IsNullOrWhiteSpace(path) || !File.Exists(path)) throw new FileNotFoundException("The verified moving-frame sequence is missing.", path);
            if ((File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0) throw new InvalidDataException("The moving-frame sequence must not be a reparse point.");
            using (var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read))
            using (var reader = new BinaryReader(file, Encoding.ASCII, true))
            {
                var magic = Encoding.ASCII.GetString(reader.ReadBytes(8));
                if (magic != "INPCSEQ2") throw new InvalidDataException("Moving-frame sequence magic is invalid.");
                var version = reader.ReadInt32();
                var frameCount = reader.ReadInt32();
                var width = reader.ReadInt32();
                var height = reader.ReadInt32();
                var framesPerSecond = reader.ReadInt32();
                var sequenceHash = Encoding.ASCII.GetString(reader.ReadBytes(64));
                var rawPixelsHash = Encoding.ASCII.GetString(reader.ReadBytes(64));
                if (version != 1 || frameCount != SyntheticScene.SourceFrameCount || width != SyntheticScene.SourceFrameWidth ||
                    height != SyntheticScene.SourceFrameHeight || framesPerSecond != SyntheticScene.SourceFrameRate ||
                    !string.Equals(sequenceHash, SyntheticScene.SourceSequenceSha256, StringComparison.Ordinal) ||
                    !string.Equals(rawPixelsHash, SyntheticScene.SourceRawPixelsSha256, StringComparison.Ordinal))
                    throw new InvalidDataException("Moving-frame sequence header does not match the pinned source contract.");
                var expectedBytes = checked(frameCount * width * height * 3);
                var decoded = new byte[expectedBytes];
                using (var inflater = new DeflateStream(file, CompressionMode.Decompress, true))
                {
                    var offset = 0;
                    while (offset < decoded.Length)
                    {
                        var read = inflater.Read(decoded, offset, decoded.Length - offset);
                        if (read == 0) break;
                        offset += read;
                    }
                    if (offset != decoded.Length || inflater.ReadByte() != -1) throw new InvalidDataException("Moving-frame sequence payload length is invalid.");
                }
                if (!string.Equals(Hashing.HashBytes(decoded), rawPixelsHash, StringComparison.Ordinal))
                    throw new InvalidDataException("Moving-frame sequence decoded payload hash mismatch.");
                return new FrameSequence(decoded, frameCount, width, height, framesPerSecond, sequenceHash, rawPixelsHash);
            }
        }

        public Bitmap DecodeFrame(int index)
        {
            if (index < 0 || index >= FrameCount) throw new ArgumentOutOfRangeException("index");
            var sourceOffset = checked(index * Width * Height * 3);
            for (var pixel = 0; pixel < Width * Height; pixel++)
            {
                var source = sourceOffset + pixel * 3;
                var destination = pixel * 3;
                bgrScratch[destination] = rgbFrames[source + 2];
                bgrScratch[destination + 1] = rgbFrames[source + 1];
                bgrScratch[destination + 2] = rgbFrames[source];
            }
            var bitmap = new Bitmap(Width, Height, PixelFormat.Format24bppRgb);
            var data = bitmap.LockBits(new Rectangle(0, 0, Width, Height), ImageLockMode.WriteOnly, PixelFormat.Format24bppRgb);
            try
            {
                if (data.Stride != Width * 3) throw new InvalidOperationException("Unexpected moving-frame bitmap stride.");
                Marshal.Copy(bgrScratch, 0, data.Scan0, bgrScratch.Length);
            }
            finally { bitmap.UnlockBits(data); }
            return bitmap;
        }
    }

    internal static class Hashing
    {
        public static string HashBytes(byte[] bytes)
        {
            using (var hash = SHA256.Create()) return Hex(hash.ComputeHash(bytes));
        }

        public static string HashFile(string path)
        {
            using (var stream = File.OpenRead(path))
            using (var hash = SHA256.Create()) return Hex(hash.ComputeHash(stream));
        }

        public static string HashBitmap(Bitmap bitmap)
        {
            var rectangle = new Rectangle(0, 0, bitmap.Width, bitmap.Height);
            var data = bitmap.LockBits(rectangle, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            try
            {
                var bytes = new byte[Math.Abs(data.Stride) * bitmap.Height];
                Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
                return HashBytes(bytes);
            }
            finally { bitmap.UnlockBits(data); }
        }

        public static string HashRegion(Bitmap bitmap, Rectangle region)
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

        private static string Hex(byte[] bytes)
        {
            var builder = new StringBuilder(bytes.Length * 2);
            foreach (var value in bytes) builder.Append(value.ToString("x2", CultureInfo.InvariantCulture));
            return builder.ToString();
        }
    }

    internal static class SyntheticScene
    {
        public const int LoopSeconds = 12;
        public const int AudioSampleRate = 22050;
        public const string PortraitResourceName = "InteractiveNpcs.SyntheticReplay.MaraVennPortraitV1.png";
        public const string PortraitSha256 = "0ae10605199bb831c34a3be29c31a06f8a1d5a7e5cd07b8a1456866f2bb7dc2d";
        public const string StaticVisualSource = "embedded-original-generated-photorealistic-portrait-v1";
        public const string MovingVisualSource = "verified-sibling-project-owned-mara-camera-sequence-v1";
        public const string MovingVisualMode = "moving-source-controlled-idle-v2";
        public const string StaticVisualMode = "static-portrait-control-v1";
        public const string MovingSequenceFileName = "mara-venn-camera-idle-v1.inpcseq";
        public const string SourceSequenceSha256 = "22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d";
        public const string SourceRawPixelsSha256 = "c58602f601d31b06cf1e006a1f65aea4e306e7983dc2b1d80d14bf4e5f094a0b";
        public const int SourceFrameCount = 42;
        public const int SourceFrameWidth = 960;
        public const int SourceFrameHeight = 720;
        public const int SourceFrameRate = 30;

        private static readonly byte[] PortraitBytes = LoadPortraitBytes();
        private static readonly Image Portrait = LoadPortrait();
        private static FrameSequence sequence;
        private static bool movingMode;

        public static bool MovingMode { get { return movingMode; } }
        public static string VisualSource { get { return movingMode ? MovingVisualSource : StaticVisualSource; } }
        public static string VisualMode { get { return movingMode ? MovingVisualMode : StaticVisualMode; } }

        public static void Configure(Options options)
        {
            var sequencePath = options.SequencePath;
            if (string.IsNullOrWhiteSpace(sequencePath)) sequencePath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, MovingSequenceFileName);
            var requestedMoving = options.PlaybackMode == "moving" || (options.PlaybackMode == "auto" && File.Exists(sequencePath));
            if (requestedMoving)
            {
                sequence = FrameSequence.Load(sequencePath);
                movingMode = true;
            }
            else
            {
                if (options.PlaybackMode == "moving") throw new FileNotFoundException("Moving mode requires the verified sibling frame sequence.", sequencePath);
                sequence = null;
                movingMode = false;
            }
        }

        public static Bitmap RenderFrame(int width, int height, double seconds)
        {
            int ignored;
            return RenderFrame(width, height, seconds, out ignored);
        }

        public static Bitmap RenderFrame(int width, int height, double seconds, out int contentFrameIndex)
        {
            if (movingMode) return RenderMovingFrame(width, height, seconds, out contentFrameIndex);
            contentFrameIndex = -1;
            return RenderStaticFrame(width, height, seconds);
        }

        public static Bitmap RenderSourceFrame(int width, int height, double seconds, out int contentFrameIndex)
        {
            if (!movingMode || sequence == null) throw new InvalidOperationException("A moving source frame is only available in moving mode.");
            contentFrameIndex = SourceIndex(seconds);
            using (var source = sequence.DecodeFrame(contentFrameIndex))
            {
                var bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb);
                using (var graphics = Graphics.FromImage(bitmap)) DrawSequenceCover(graphics, source, width, height);
                return bitmap;
            }
        }

        private static Bitmap RenderMovingFrame(int width, int height, double seconds, out int contentFrameIndex)
        {
            var bitmap = RenderSourceFrame(width, height, seconds, out contentFrameIndex);
            using (var immutableBase = (Bitmap)bitmap.Clone())
            using (var graphics = Graphics.FromImage(bitmap))
            {
                graphics.SmoothingMode = SmoothingMode.HighQuality;
                graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                graphics.CompositingQuality = CompositingQuality.HighQuality;
                var scaleX = width / 960.0f;
                var scaleY = height / 600.0f;
                ApplyBreathing(graphics, immutableBase, scaleX, scaleY, BreathingAmount(seconds));
                var blink = BlinkAmount(seconds);
                ApplyBlink(bitmap, immutableBase, Scale(new RectangleF(407, 177, 85, 34), scaleX, scaleY), blink);
                ApplyBlink(bitmap, immutableBase, Scale(new RectangleF(514, 177, 85, 34), scaleX, scaleY), blink);
                DrawMovingChrome(graphics, width, height, seconds, contentFrameIndex);
            }
            return bitmap;
        }

        private static void DrawSequenceCover(Graphics graphics, Image image, int width, int height)
        {
            graphics.Clear(Color.Black);
            graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            graphics.DrawImage(image, new Rectangle(0, 0, width, height), new Rectangle(0, 60, 960, 600), GraphicsUnit.Pixel);
        }

        private static void ApplyBreathing(Graphics graphics, Bitmap source, float scaleX, float scaleY, double breath)
        {
            var top = (int)Math.Round(405 * scaleY);
            var bottom = source.Height;
            const int bandCount = 10;
            for (var band = 0; band < bandCount; band++)
            {
                var sourceTop = top + (bottom - top) * band / bandCount;
                var sourceBottom = top + (bottom - top) * (band + 1) / bandCount;
                var progress = (band + 1.0) / bandCount;
                var horizontal = (float)((breath - 0.5) * 5.0 * progress * scaleX);
                var vertical = (float)((breath - 0.5) * 6.0 * progress * scaleY);
                var sourceBand = new Rectangle(0, sourceTop, source.Width, Math.Max(1, sourceBottom - sourceTop));
                var destination = new RectangleF(-horizontal, sourceTop + vertical, source.Width + horizontal * 2, sourceBand.Height + 1.2f * scaleY);
                graphics.DrawImage(source, destination, sourceBand, GraphicsUnit.Pixel);
            }
        }

        private static void ApplyBlink(Bitmap destination, Bitmap source, RectangleF region, double blink)
        {
            if (blink <= 0.001) return;
            var left = Math.Max(0, (int)Math.Floor(region.Left));
            var top = Math.Max(0, (int)Math.Floor(region.Top));
            var right = Math.Min(destination.Width, (int)Math.Ceiling(region.Right));
            var bottom = Math.Min(destination.Height, (int)Math.Ceiling(region.Bottom));
            var centerX = region.Left + region.Width * 0.5;
            var centerY = region.Top + region.Height * 0.52;
            var radiusX = region.Width * 0.5;
            var radiusY = region.Height * 0.5;
            var closure = region.Height * 0.235 * blink;
            for (var y = top; y < bottom; y++)
            {
                for (var x = left; x < right; x++)
                {
                    var normalizedX = (x + 0.5 - centerX) / radiusX;
                    var normalizedY = (y + 0.5 - centerY) / radiusY;
                    var radius = normalizedX * normalizedX + normalizedY * normalizedY;
                    if (radius >= 1.0) continue;
                    var feather = Math.Min(1.0, (1.0 - radius) / 0.34);
                    var curve = region.Height * 0.035 * normalizedX * normalizedX;
                    var localCenterY = centerY + curve;
                    var upperY = Math.Max(0, Math.Min(source.Height - 1, (int)Math.Round(y - closure)));
                    var lowerY = Math.Max(0, Math.Min(source.Height - 1, (int)Math.Round(y + closure)));
                    var upper = source.GetPixel(x, upperY);
                    var lower = source.GetPixel(x, lowerY);
                    var transition = Math.Max(0, Math.Min(1, 0.5 + (y - localCenterY) / (region.Height * 0.16)));
                    transition = transition * transition * (3.0 - 2.0 * transition);
                    var sample = Blend(upper, lower, transition);
                    var original = destination.GetPixel(x, y);
                    var weight = blink * feather;
                    var result = Blend(original, sample, weight);
                    if (Math.Abs(y - localCenterY) < 0.75 && Math.Abs(normalizedX) < 0.68)
                        result = Blend(result, Color.FromArgb(255, 68, 48, 43), blink * feather * 0.08);
                    destination.SetPixel(x, y, result);
                }
            }
        }

        private static Color Blend(Color from, Color to, double amount)
        {
            amount = Math.Max(0, Math.Min(1, amount));
            return Color.FromArgb(
                255,
                (int)Math.Round(from.R + (to.R - from.R) * amount),
                (int)Math.Round(from.G + (to.G - from.G) * amount),
                (int)Math.Round(from.B + (to.B - from.B) * amount));
        }

        private static void DrawMovingChrome(Graphics graphics, int width, int height, double seconds, int frameIndex)
        {
            graphics.ScaleTransform(width / 960.0f, height / 600.0f);
            using (var panelPath = new GraphicsPath())
            {
                panelPath.AddPolygon(new[] { new PointF(22, 20), new PointF(322, 20), new PointF(342, 40), new PointF(342, 98), new PointF(22, 98) });
                using (var panel = new SolidBrush(Color.FromArgb(196, 5, 10, 20))) graphics.FillPath(panel, panelPath);
                using (var border = new Pen(Color.FromArgb(210, 83, 224, 219), 1.25f)) graphics.DrawPath(border, panelPath);
            }
            using (var title = new Font("Segoe UI Semibold", 19, FontStyle.Bold, GraphicsUnit.Point))
            using (var small = new Font("Consolas", 8.5f, FontStyle.Regular, GraphicsUnit.Point))
            using (var titleBrush = new SolidBrush(Color.FromArgb(255, 244, 238, 226)))
            using (var cyan = new SolidBrush(Color.FromArgb(235, 103, 237, 231)))
            using (var muted = new SolidBrush(Color.FromArgb(210, 196, 210, 214)))
            {
                graphics.DrawString("ECLIPSE HARBOR", title, titleBrush, 39, 30);
                graphics.DrawString("CAPTURE FIXTURE  /  MARA VENN", small, cyan, 41, 70);
                graphics.DrawString("FRAME " + (frameIndex + 1).ToString("00", CultureInfo.InvariantCulture) + "/42", small, muted, 250, 70);
            }
            using (var marker = new SolidBrush(Color.FromArgb((int)(155 + 100 * (0.5 + 0.5 * Math.Sin(seconds * Math.PI * 2.0))), 255, 98, 114)))
                graphics.FillEllipse(marker, 913, 28, 16, 16);
            using (var strip = new SolidBrush(Color.FromArgb(164, 4, 8, 17))) graphics.FillRectangle(strip, 196, 558, 742, 24);
            using (var line = new Pen(Color.FromArgb(170, 83, 224, 219), 1.0f)) graphics.DrawLine(line, 196, 558, 938, 558);
            using (var footer = new Font("Consolas", 7.8f, FontStyle.Regular, GraphicsUnit.Point))
            using (var footerBrush = new SolidBrush(Color.FromArgb(230, 224, 229, 226)))
                graphics.DrawString("42-FRAME CAMERA SOURCE  •  SYNTHETIC BLINK + BREATH  •  MOUTH UNARTICULATED", footer, footerBrush, 210, 563);
            graphics.ResetTransform();
        }

        private static Bitmap RenderStaticFrame(int width, int height, double seconds)
        {
            var bitmap = new Bitmap(width, height, PixelFormat.Format32bppArgb);
            using (var graphics = Graphics.FromImage(bitmap))
            {
                graphics.SmoothingMode = SmoothingMode.AntiAlias;
                graphics.TextRenderingHint = System.Drawing.Text.TextRenderingHint.ClearTypeGridFit;
                graphics.ScaleTransform(width / 960.0f, height / 600.0f);
                var phase = (seconds % LoopSeconds) / LoopSeconds;
                var beacon = 0.5f + 0.5f * (float)Math.Sin(phase * Math.PI * 8.0);
                DrawCover(graphics, Portrait, new Rectangle(0, 0, 960, 600));
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
                    graphics.FillRectangle(panel, 24, 22, 330, 88);
                    graphics.DrawRectangle(border, 24, 22, 330, 88);
                }
                using (var title = new Font("Segoe UI Semibold", 19, FontStyle.Bold, GraphicsUnit.Point))
                using (var small = new Font("Consolas", 9, FontStyle.Regular, GraphicsUnit.Point))
                using (var titleBrush = new SolidBrush(Color.FromArgb(255, 244, 225)))
                using (var mutedBrush = new SolidBrush(Color.FromArgb(205, 210, 202, 208)))
                {
                    graphics.DrawString("ECLIPSE HARBOR", title, titleBrush, 40, 34);
                    graphics.DrawString("STATIC PORTRAIT CONTROL  //  MOUTH INVARIANT", small, mutedBrush, 42, 78);
                }
                using (var beaconBrush = new SolidBrush(Color.FromArgb((int)(80 + beacon * 175), 255, 94, 115))) graphics.FillEllipse(beaconBrush, 905, 28, 20, 20);
            }
            return bitmap;
        }

        public static Rectangle MouthGuardRectangle(int width, int height)
        {
            return new Rectangle((int)Math.Round(width * 0.455), (int)Math.Round(height * 0.455), Math.Max(1, (int)Math.Round(width * 0.115)), Math.Max(1, (int)Math.Round(height * 0.105)));
        }

        public static Rectangle EyeGuardRectangle(int width, int height)
        {
            return Rectangle.Round(Scale(new RectangleF(404, 174, 198, 40), width / 960.0f, height / 600.0f));
        }

        public static Rectangle TorsoGuardRectangle(int width, int height)
        {
            return Rectangle.Round(Scale(new RectangleF(220, 430, 520, 145), width / 960.0f, height / 600.0f));
        }

        public static double BlinkAmount(double seconds)
        {
            var cycle = seconds % 8.0;
            return Math.Max(Pulse(cycle, 2.20, 0.18), Pulse(cycle, 6.35, 0.15));
        }

        public static double BreathingAmount(double seconds)
        {
            return 0.5 + 0.5 * Math.Sin((seconds / 4.8) * Math.PI * 2.0 - Math.PI * 0.5);
        }

        public static int SourceIndex(double seconds)
        {
            var forward = SourceFrameCount - 1;
            var cycle = forward * 2;
            var step = ((long)Math.Floor(Math.Max(0, seconds) * SourceFrameRate)) % cycle;
            return (int)(step <= forward ? step : cycle - step);
        }

        public static string EmbeddedPortraitHash() { return Hashing.HashBytes(PortraitBytes); }

        private static double Pulse(double value, double center, double halfWidth)
        {
            var amount = 1.0 - Math.Abs(value - center) / halfWidth;
            if (amount <= 0) return 0;
            if (amount >= 1) return 1;
            return amount * amount * (3.0 - 2.0 * amount);
        }

        private static RectangleF Scale(RectangleF rectangle, float scaleX, float scaleY)
        {
            return new RectangleF(rectangle.X * scaleX, rectangle.Y * scaleY, rectangle.Width * scaleX, rectangle.Height * scaleY);
        }

        private static void DrawCover(Graphics graphics, Image image, Rectangle destination)
        {
            var scale = Math.Max((double)destination.Width / image.Width, (double)destination.Height / image.Height);
            var sourceWidth = destination.Width / scale;
            var sourceHeight = destination.Height / scale;
            var source = new RectangleF((float)((image.Width - sourceWidth) / 2.0), (float)((image.Height - sourceHeight) / 2.0), (float)sourceWidth, (float)sourceHeight);
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
            var actualHash = Hashing.HashBytes(PortraitBytes);
            if (!string.Equals(actualHash, PortraitSha256, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Embedded portrait hash mismatch.");
            using (var stream = new MemoryStream(PortraitBytes, false))
            using (var decoded = Image.FromStream(stream, true, true)) return new Bitmap(decoded);
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
            metrics = new AudioMetrics { DurationSeconds = LoopSeconds, SampleRate = AudioSampleRate, SampleCount = sampleCount, Peak = peak, Rms = Math.Sqrt(sumSquares / sampleCount), NonzeroSamples = nonzero, ClippedSamples = 0, LoopBoundaryDelta = Math.Abs((int)pcm[0] - (int)pcm[pcm.Length - 1]) };
            using (var stream = new MemoryStream())
            using (var writer = new BinaryWriter(stream, Encoding.ASCII))
            {
                writer.Write(Encoding.ASCII.GetBytes("RIFF"));
                writer.Write(36 + sampleCount * 2);
                writer.Write(Encoding.ASCII.GetBytes("WAVEfmt "));
                writer.Write(16); writer.Write((short)1); writer.Write((short)1); writer.Write(AudioSampleRate); writer.Write(AudioSampleRate * 2); writer.Write((short)2); writer.Write((short)16);
                writer.Write(Encoding.ASCII.GetBytes("data")); writer.Write(sampleCount * 2);
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
        private string currentFrameSha256;
        private int currentContentFrameIndex = -1;
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
            if (!options.Loop && frameIndex >= (long)Math.Round(SyntheticScene.LoopSeconds * options.FramesPerSecond)) { Close(); return; }
            GenerateFrame(frameIndex);
            if (frameIndex % Math.Max(1, (int)Math.Round(options.FramesPerSecond)) == 0) WriteMetadata("playing", null);
        }

        private void GenerateFrame(long frameIndex)
        {
            int contentIndex;
            var next = SyntheticScene.RenderFrame(options.Width, options.Height, frameIndex / options.FramesPerSecond, out contentIndex);
            var nextHash = Hashing.HashBitmap(next);
            lock (frameGate)
            {
                var previous = currentFrame;
                currentFrame = next;
                currentContentFrameIndex = contentIndex;
                currentFrameSha256 = nextHash;
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
            string frameHash;
            int contentIndex;
            lock (frameGate) { frameHash = currentFrameSha256; contentIndex = currentContentFrameIndex; }
            var payload = new Dictionary<string, object> {
                { "schema_version", SyntheticScene.MovingMode ? 2 : 1 },
                { "fixture_kind", "synthetic-original-video-replay" }, { "fixture_id", "eclipse-harbor-local-review" },
                { "fixture_source", SyntheticScene.MovingMode ? "project-source-generated-native-v2" : "project-source-generated-native-v1" },
                { "state", state }, { "playback_state", state }, { "generated_at_utc", DateTime.UtcNow.ToString("o", CultureInfo.InvariantCulture) },
                { "pid", process.Id }, { "process_id", process.Id }, { "window_handle", Handle.ToInt64() }, { "hwnd", Handle.ToInt64() },
                { "hwnd_hex", "0x" + Handle.ToInt64().ToString("X", CultureInfo.InvariantCulture) },
                { "executable_basename", Path.GetFileName(process.MainModule.FileName) }, { "exe_basename", Path.GetFileName(process.MainModule.FileName) },
                { "window_title", Text }, { "width", options.Width }, { "height", options.Height }, { "frames_per_second", options.FramesPerSecond },
                { "loop", options.Loop }, { "generated_frames", Interlocked.Read(ref generatedFrames) }, { "decoded_frames", Interlocked.Read(ref generatedFrames) },
                { "renderer", SyntheticScene.MovingMode ? "project-owned-gdi-source-pixel-idle-v2" : "project-owned-gdi-static-control-v1" },
                { "decoder", SyntheticScene.MovingMode ? "inbox-deflate-sequence-v1" : "none-generated-frames" },
                { "visual_source", SyntheticScene.VisualSource }, { "visual_mode", SyntheticScene.VisualMode }, { "portrait_sha256", SyntheticScene.PortraitSha256 },
                { "source_sequence_sha256", SyntheticScene.MovingMode ? SyntheticScene.SourceSequenceSha256 : null },
                { "source_frame_count", SyntheticScene.MovingMode ? SyntheticScene.SourceFrameCount : 1 },
                { "source_frame_width", SyntheticScene.MovingMode ? SyntheticScene.SourceFrameWidth : 1536 },
                { "source_frame_height", SyntheticScene.MovingMode ? SyntheticScene.SourceFrameHeight : 1024 },
                { "source_frame_rate", SyntheticScene.MovingMode ? SyntheticScene.SourceFrameRate : 0 },
                { "source_frame_motion", SyntheticScene.MovingMode }, { "source_actor_motion", false },
                { "rendered_actor_motion", SyntheticScene.MovingMode }, { "rendered_blink_motion", SyntheticScene.MovingMode }, { "rendered_breathing_motion", SyntheticScene.MovingMode },
                { "source_mouth_motion", false }, { "source_mouth_articulation", false }, { "product_lip_sync", false },
                { "content_frame_index", contentIndex }, { "content_frame_sha256", frameHash },
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

    internal static class HeadlessSequenceExport
    {
        public static int Run(Options options)
        {
            if (!SyntheticScene.MovingMode) throw new InvalidOperationException("Headless sequence export requires the verified moving source.");
            Directory.CreateDirectory(options.HeadlessExportDirectory);
            var framesDirectory = Path.Combine(options.HeadlessExportDirectory, "frames-rgb");
            Directory.CreateDirectory(framesDirectory);
            object[] sourceLandmarks = null;
            string landmarksHash = null;
            if (!string.IsNullOrWhiteSpace(options.SourceMouthLandmarksPath))
            {
                if (!File.Exists(options.SourceMouthLandmarksPath)) throw new FileNotFoundException("Source mouth landmarks are missing.", options.SourceMouthLandmarksPath);
                var root = new JavaScriptSerializer().DeserializeObject(File.ReadAllText(options.SourceMouthLandmarksPath, Encoding.UTF8)) as Dictionary<string, object>;
                if (root == null || !root.ContainsKey("frames")) throw new InvalidDataException("Source mouth landmark JSON is invalid.");
                sourceLandmarks = root["frames"] as object[];
                if (sourceLandmarks == null || sourceLandmarks.Length != SyntheticScene.SourceFrameCount)
                    throw new InvalidDataException("Source mouth landmarks do not cover the pinned 42-frame sequence.");
                landmarksHash = Hashing.HashFile(options.SourceMouthLandmarksPath);
            }

            var records = new List<Dictionary<string, object>>();
            var sourceIndices = new HashSet<int>();
            var renderedHashes = new HashSet<string>(StringComparer.Ordinal);
            var mouthGuard = SyntheticScene.MouthGuardRectangle(options.Width, options.Height);
            var mouthPixelsPreserved = true;
            for (var index = 0; index < options.HeadlessExportFrameCount; index++)
            {
                var seconds = options.HeadlessExportStartSeconds + index / options.FramesPerSecond;
                int sourceIndex;
                int contentIndex;
                using (var source = SyntheticScene.RenderSourceFrame(options.Width, options.Height, seconds, out sourceIndex))
                using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, seconds, out contentIndex))
                {
                    if (sourceIndex != contentIndex) throw new InvalidOperationException("Exported frame advanced to a different pinned source frame.");
                    var path = Path.Combine(framesDirectory, "frame-" + index.ToString("00000", CultureInfo.InvariantCulture) + ".ppm");
                    SavePpm(frame, path);
                    var sourceMouthHash = Hashing.HashRegion(source, mouthGuard);
                    var renderedMouthHash = Hashing.HashRegion(frame, mouthGuard);
                    mouthPixelsPreserved &= sourceMouthHash == renderedMouthHash;
                    var bitmapHash = Hashing.HashBitmap(frame);
                    renderedHashes.Add(bitmapHash);
                    sourceIndices.Add(contentIndex);
                    var record = new Dictionary<string, object> {
                        { "frame_index", index }, { "seconds", seconds }, { "content_frame_index", contentIndex },
                        { "file", "frames-rgb/" + Path.GetFileName(path) }, { "file_sha256", Hashing.HashFile(path) },
                        { "rendered_argb_sha256", bitmapHash }, { "blink_amount", SyntheticScene.BlinkAmount(seconds) },
                        { "breathing_amount", SyntheticScene.BreathingAmount(seconds) },
                        { "mouth_guard", RectanglePayload(mouthGuard) }, { "source_mouth_sha256", sourceMouthHash },
                        { "rendered_mouth_sha256", renderedMouthHash }, { "mouth_pixels_preserved", sourceMouthHash == renderedMouthHash }
                    };
                    if (sourceLandmarks != null) record["mouth_geometry"] = TransformGeometry(sourceLandmarks[contentIndex], options.Width, options.Height);
                    records.Add(record);
                }
            }

            int firstIndexA;
            int firstIndexB;
            string firstHashA;
            string firstHashB;
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, options.HeadlessExportStartSeconds, out firstIndexA)) firstHashA = Hashing.HashBitmap(frame);
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, options.HeadlessExportStartSeconds, out firstIndexB)) firstHashB = Hashing.HashBitmap(frame);
            var deterministic = firstIndexA == firstIndexB && firstHashA == firstHashB;
            var passed = mouthPixelsPreserved && deterministic && sourceIndices.Count > 1 && renderedHashes.Count > 1;
            var payload = new Dictionary<string, object> {
                { "schema", "interactive-npcs-headless-pre-lip-sequence/v1" }, { "status", passed ? "passed" : "failed" },
                { "render_stage", "post-controlled-blink-and-breath-pre-lip-articulation" },
                { "visual_source", SyntheticScene.MovingVisualSource }, { "visual_mode", SyntheticScene.MovingVisualMode },
                { "source_sequence_sha256", SyntheticScene.SourceSequenceSha256 }, { "source_raw_pixels_sha256", SyntheticScene.SourceRawPixelsSha256 },
                { "source_frame_count", SyntheticScene.SourceFrameCount }, { "source_frame_width", SyntheticScene.SourceFrameWidth },
                { "source_frame_height", SyntheticScene.SourceFrameHeight }, { "source_frame_rate", SyntheticScene.SourceFrameRate },
                { "rendered_width", options.Width }, { "rendered_height", options.Height }, { "rendered_frame_rate", options.FramesPerSecond },
                { "export_frame_count", options.HeadlessExportFrameCount }, { "start_seconds", options.HeadlessExportStartSeconds },
                { "duration_seconds", options.HeadlessExportFrameCount / options.FramesPerSecond },
                { "pixel_format", "ppm-p6-rgb24" }, { "source_frame_motion", true }, { "source_actor_motion", false },
                { "rendered_actor_motion", true }, { "rendered_blink_motion", true }, { "rendered_breathing_motion", true },
                { "source_mouth_articulation", false }, { "product_lip_sync", false },
                { "renderer_mouth_pixels_preserved", mouthPixelsPreserved }, { "deterministic_rendering", deterministic },
                { "distinct_source_indices", sourceIndices.Count }, { "distinct_rendered_frame_hashes", renderedHashes.Count },
                { "mouth_geometry_source_path", options.SourceMouthLandmarksPath }, { "mouth_geometry_source_sha256", landmarksHash },
                { "mouth_geometry_transform", "x=source_normalized_x*rendered_width; y=(source_normalized_y*720-60)*rendered_height/600" },
                { "viewport_source_crop", new Dictionary<string, object> { { "x", 0 }, { "y", 60 }, { "width", 960 }, { "height", 600 } } },
                { "audio_output", "not-created" }, { "window_created", false }, { "frames", records }
            };
            var reportPath = Path.Combine(options.HeadlessExportDirectory, "sequence.json");
            File.WriteAllText(reportPath, new JavaScriptSerializer().Serialize(payload), new UTF8Encoding(false));
            return passed ? 0 : 2;
        }

        private static Dictionary<string, object> TransformGeometry(object sourceObject, int width, int height)
        {
            var source = sourceObject as Dictionary<string, object>;
            if (source == null || !Convert.ToBoolean(source["accepted"], CultureInfo.InvariantCulture))
                throw new InvalidDataException("A required source mouth landmark frame was not accepted.");
            var result = new Dictionary<string, object> {
                { "source_file", Convert.ToString(source["file"], CultureInfo.InvariantCulture) },
                { "accepted", true }, { "center", TransformPoint(source["center"], width, height) },
                { "bounds", TransformBounds(source["bounds"], width, height) },
                { "corner_width_pixels", Convert.ToDouble(source["cornerWidth"], CultureInfo.InvariantCulture) * width },
                { "roll_radians", Convert.ToDouble(source["rollRadians"], CultureInfo.InvariantCulture) },
                { "outer_upper", TransformPoints(source["outerUpper"], width, height) },
                { "outer_lower", TransformPoints(source["outerLower"], width, height) },
                { "inner_upper", TransformPoints(source["innerUpper"], width, height) },
                { "inner_lower", TransformPoints(source["innerLower"], width, height) }
            };
            return result;
        }

        private static object[] TransformPoints(object value, int width, int height)
        {
            var points = value as object[];
            if (points == null) throw new InvalidDataException("Source mouth landmark point list is invalid.");
            var transformed = new object[points.Length];
            for (var index = 0; index < points.Length; index++) transformed[index] = TransformPoint(points[index], width, height);
            return transformed;
        }

        private static object[] TransformPoint(object value, int width, int height)
        {
            var point = value as object[];
            if (point == null || point.Length < 2) throw new InvalidDataException("Source mouth landmark point is invalid.");
            var x = Convert.ToDouble(point[0], CultureInfo.InvariantCulture) * width;
            var y = (Convert.ToDouble(point[1], CultureInfo.InvariantCulture) * 720.0 - 60.0) * height / 600.0;
            if (point.Length > 2) return new object[] { x, y, Convert.ToDouble(point[2], CultureInfo.InvariantCulture) * width };
            return new object[] { x, y };
        }

        private static object[] TransformBounds(object value, int width, int height)
        {
            var bounds = value as object[];
            if (bounds == null || bounds.Length != 4) throw new InvalidDataException("Source mouth landmark bounds are invalid.");
            return new object[] {
                Convert.ToDouble(bounds[0], CultureInfo.InvariantCulture) * width,
                (Convert.ToDouble(bounds[1], CultureInfo.InvariantCulture) * 720.0 - 60.0) * height / 600.0,
                Convert.ToDouble(bounds[2], CultureInfo.InvariantCulture) * width,
                Convert.ToDouble(bounds[3], CultureInfo.InvariantCulture) * 720.0 * height / 600.0
            };
        }

        private static Dictionary<string, object> RectanglePayload(Rectangle rectangle)
        {
            return new Dictionary<string, object> { { "x", rectangle.X }, { "y", rectangle.Y }, { "width", rectangle.Width }, { "height", rectangle.Height } };
        }

        private static void SavePpm(Bitmap bitmap, string path)
        {
            var rectangle = new Rectangle(Point.Empty, bitmap.Size);
            var data = bitmap.LockBits(rectangle, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            try
            {
                var packed = new byte[checked(bitmap.Width * bitmap.Height * 3)];
                var row = new byte[Math.Abs(data.Stride)];
                var offset = 0;
                for (var y = 0; y < bitmap.Height; y++)
                {
                    var rowPointer = IntPtr.Add(data.Scan0, y * data.Stride);
                    Marshal.Copy(rowPointer, row, 0, row.Length);
                    for (var x = 0; x < bitmap.Width; x++)
                    {
                        var pixel = x * 4;
                        packed[offset++] = row[pixel + 2];
                        packed[offset++] = row[pixel + 1];
                        packed[offset++] = row[pixel];
                    }
                }
                using (var stream = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.None))
                {
                    var header = Encoding.ASCII.GetBytes("P6\n" + bitmap.Width.ToString(CultureInfo.InvariantCulture) + " " + bitmap.Height.ToString(CultureInfo.InvariantCulture) + "\n255\n");
                    stream.Write(header, 0, header.Length);
                    stream.Write(packed, 0, packed.Length);
                }
            }
            finally { bitmap.UnlockBits(data); }
        }
    }

    internal static class SelfTest
    {
        public static int Run(Options options)
        {
            return SyntheticScene.MovingMode ? RunMoving(options) : RunStatic(options);
        }

        private static int RunMoving(Options options)
        {
            var frameDirectory = options.SelfTestFrameDirectory ?? Path.Combine(Path.GetDirectoryName(options.SelfTestReportPath), "frames");
            Directory.CreateDirectory(frameDirectory);
            var samples = new[] { 0.0, 0.75, 2.20, 3.60, 6.35, 7.50 };
            var frameRecords = new List<Dictionary<string, object>>();
            var fullHashes = new HashSet<string>(StringComparer.Ordinal);
            var sourceIndices = new HashSet<int>();
            var mouthPreserved = true;
            string openEyeHash = null;
            string blinkEyeHash = null;
            string restingTorsoHash = null;
            string expandedTorsoHash = null;
            var mouthGuard = SyntheticScene.MouthGuardRectangle(options.Width, options.Height);
            var eyeGuard = SyntheticScene.EyeGuardRectangle(options.Width, options.Height);
            var torsoGuard = SyntheticScene.TorsoGuardRectangle(options.Width, options.Height);
            for (var sampleIndex = 0; sampleIndex < samples.Length; sampleIndex++)
            {
                var seconds = samples[sampleIndex];
                int contentIndex;
                int sourceIndex;
                using (var source = SyntheticScene.RenderSourceFrame(options.Width, options.Height, seconds, out sourceIndex))
                using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, seconds, out contentIndex))
                {
                    if (sourceIndex != contentIndex) throw new InvalidOperationException("Rendered frame advanced to a different pinned source frame.");
                    var fullPath = Path.Combine(frameDirectory, "moving-full-" + sampleIndex.ToString("00", CultureInfo.InvariantCulture) + ".png");
                    var facePath = Path.Combine(frameDirectory, "moving-face-" + sampleIndex.ToString("00", CultureInfo.InvariantCulture) + ".png");
                    var mouthPath = Path.Combine(frameDirectory, "moving-mouth-" + sampleIndex.ToString("00", CultureInfo.InvariantCulture) + ".png");
                    frame.Save(fullPath, ImageFormat.Png);
                    SaveCrop(frame, new Rectangle((int)(options.Width * 0.31), (int)(options.Height * 0.18), (int)(options.Width * 0.38), (int)(options.Height * 0.48)), facePath);
                    SaveCrop(frame, mouthGuard, mouthPath);
                    var fullHash = Hashing.HashFile(fullPath);
                    var sourceMouthHash = Hashing.HashRegion(source, mouthGuard);
                    var outputMouthHash = Hashing.HashRegion(frame, mouthGuard);
                    mouthPreserved &= string.Equals(sourceMouthHash, outputMouthHash, StringComparison.Ordinal);
                    fullHashes.Add(fullHash);
                    sourceIndices.Add(contentIndex);
                    var eyeHash = Hashing.HashRegion(frame, eyeGuard);
                    var torsoHash = Hashing.HashRegion(frame, torsoGuard);
                    if (sampleIndex == 0) { openEyeHash = eyeHash; restingTorsoHash = torsoHash; }
                    if (sampleIndex == 2) blinkEyeHash = eyeHash;
                    if (sampleIndex == 3) expandedTorsoHash = torsoHash;
                    frameRecords.Add(new Dictionary<string, object> {
                        { "sample_index", sampleIndex }, { "seconds", seconds }, { "content_frame_index", contentIndex },
                        { "blink_amount", SyntheticScene.BlinkAmount(seconds) }, { "breathing_amount", SyntheticScene.BreathingAmount(seconds) },
                        { "full_path", fullPath }, { "full_sha256", fullHash }, { "face_path", facePath }, { "face_sha256", Hashing.HashFile(facePath) },
                        { "mouth_path", mouthPath }, { "mouth_sha256", Hashing.HashFile(mouthPath) },
                        { "source_mouth_sha256", sourceMouthHash }, { "rendered_mouth_sha256", outputMouthHash }, { "mouth_pixels_preserved", sourceMouthHash == outputMouthHash }
                    });
                }
            }
            int deterministicIndexA;
            int deterministicIndexB;
            string deterministicHashA;
            string deterministicHashB;
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, 2.20, out deterministicIndexA)) deterministicHashA = Hashing.HashBitmap(frame);
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, 2.20, out deterministicIndexB)) deterministicHashB = Hashing.HashBitmap(frame);
            AudioMetrics audio;
            var waveHash = Hashing.HashBytes(SyntheticScene.CreateWave(out audio));
            var sourceAdvancePassed = sourceIndices.Count >= 5;
            var blinkPassed = !string.Equals(openEyeHash, blinkEyeHash, StringComparison.Ordinal);
            var breathingPassed = !string.Equals(restingTorsoHash, expandedTorsoHash, StringComparison.Ordinal);
            var deterministicPassed = deterministicIndexA == deterministicIndexB && deterministicHashA == deterministicHashB;
            var audioPassed = audio.NonzeroSamples > audio.SampleCount / 2 && audio.Peak > 256 && audio.Peak < short.MaxValue && audio.Rms > 128 && audio.ClippedSamples == 0 && audio.LoopBoundaryDelta < 256;
            var portraitHash = SyntheticScene.EmbeddedPortraitHash();
            var portraitVerified = string.Equals(portraitHash, SyntheticScene.PortraitSha256, StringComparison.OrdinalIgnoreCase);
            var passed = sourceAdvancePassed && blinkPassed && breathingPassed && deterministicPassed && mouthPreserved && fullHashes.Count == samples.Length && audioPassed && portraitVerified;
            var payload = new Dictionary<string, object> {
                { "schema_version", 2 }, { "status", passed ? "passed" : "failed" }, { "fixture_source", "project-source-generated-native-v2" },
                { "renderer", "project-owned-gdi-source-pixel-idle-v2" }, { "visual_source", SyntheticScene.MovingVisualSource }, { "visual_mode", SyntheticScene.MovingVisualMode },
                { "portrait_sha256", portraitHash }, { "portrait_hash_verified", portraitVerified },
                { "source_sequence_sha256", SyntheticScene.SourceSequenceSha256 }, { "source_raw_pixels_sha256", SyntheticScene.SourceRawPixelsSha256 },
                { "source_frame_count", SyntheticScene.SourceFrameCount }, { "source_frame_width", SyntheticScene.SourceFrameWidth },
                { "source_frame_height", SyntheticScene.SourceFrameHeight }, { "source_frame_rate", SyntheticScene.SourceFrameRate },
                { "source_frame_motion", true }, { "source_actor_motion", false }, { "source_camera_motion_only", true },
                { "rendered_actor_motion", true }, { "rendered_blink_motion", blinkPassed }, { "rendered_breathing_motion", breathingPassed },
                { "source_mouth_motion", false }, { "source_mouth_articulation", false }, { "product_lip_sync", false },
                { "renderer_mouth_pixels_preserved", mouthPreserved }, { "deterministic_rendering", deterministicPassed },
                { "source_advance_passed", sourceAdvancePassed }, { "distinct_source_indices", sourceIndices.Count },
                { "generated_probe_frames", samples.Length }, { "distinct_rendered_frame_hashes", fullHashes.Count },
                { "eye_region_open_sha256", openEyeHash }, { "eye_region_blink_sha256", blinkEyeHash },
                { "torso_region_rest_sha256", restingTorsoHash }, { "torso_region_expanded_sha256", expandedTorsoHash },
                { "frames", frameRecords },
                { "audio_source", "project-owned-generated-pcm-v1" }, { "audio_sha256", waveHash }, { "audio_duration_seconds", audio.DurationSeconds },
                { "audio_sample_rate", audio.SampleRate }, { "audio_sample_count", audio.SampleCount }, { "audio_nonzero_samples", audio.NonzeroSamples },
                { "audio_peak", audio.Peak }, { "audio_rms", audio.Rms }, { "audio_clipped_samples", audio.ClippedSamples },
                { "audio_loop_boundary_delta", audio.LoopBoundaryDelta }, { "hardware_acceleration", false }, { "nvidia_compute_requested", false },
                { "third_party_binaries_loaded", false }
            };
            WriteJson(options.SelfTestReportPath, payload);
            return passed ? 0 : 2;
        }

        private static int RunStatic(Options options)
        {
            var frameDirectory = options.SelfTestFrameDirectory ?? Path.Combine(Path.GetDirectoryName(options.SelfTestReportPath), "frames");
            Directory.CreateDirectory(frameDirectory);
            var frameAPath = Path.Combine(frameDirectory, "static-frame-000.png");
            var frameBPath = Path.Combine(frameDirectory, "static-frame-045.png");
            string frameAHash;
            string frameBHash;
            string mouthAHash;
            string mouthBHash;
            var mouthGuard = SyntheticScene.MouthGuardRectangle(options.Width, options.Height);
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, 0)) { frame.Save(frameAPath, ImageFormat.Png); frameAHash = Hashing.HashFile(frameAPath); mouthAHash = Hashing.HashRegion(frame, mouthGuard); }
            using (var frame = SyntheticScene.RenderFrame(options.Width, options.Height, 3)) { frame.Save(frameBPath, ImageFormat.Png); frameBHash = Hashing.HashFile(frameBPath); mouthBHash = Hashing.HashRegion(frame, mouthGuard); }
            AudioMetrics audio;
            var waveHash = Hashing.HashBytes(SyntheticScene.CreateWave(out audio));
            var framesDiffer = !string.Equals(frameAHash, frameBHash, StringComparison.OrdinalIgnoreCase);
            var mouthRegionInvariant = string.Equals(mouthAHash, mouthBHash, StringComparison.OrdinalIgnoreCase);
            var audioPassed = audio.NonzeroSamples > audio.SampleCount / 2 && audio.Peak > 256 && audio.Peak < short.MaxValue && audio.Rms > 128 && audio.ClippedSamples == 0 && audio.LoopBoundaryDelta < 256;
            var portraitHash = SyntheticScene.EmbeddedPortraitHash();
            var portraitVerified = string.Equals(portraitHash, SyntheticScene.PortraitSha256, StringComparison.OrdinalIgnoreCase);
            var passed = framesDiffer && mouthRegionInvariant && portraitVerified && audioPassed;
            var payload = new Dictionary<string, object> {
                { "schema_version", 1 }, { "status", passed ? "passed" : "failed" }, { "fixture_source", "project-source-generated-native-v1" },
                { "renderer", "project-owned-gdi-generated-v1" }, { "generated_frame_count", 2 }, { "generated_probe_frames", 2 }, { "frames_differ", framesDiffer },
                { "visual_source", SyntheticScene.StaticVisualSource }, { "visual_mode", SyntheticScene.StaticVisualMode },
                { "portrait_sha256", portraitHash }, { "portrait_hash_verified", portraitVerified },
                { "source_mouth_motion", false }, { "source_mouth_articulation", false }, { "product_lip_sync", false },
                { "mouth_guard_x", mouthGuard.X }, { "mouth_guard_y", mouthGuard.Y }, { "mouth_guard_width", mouthGuard.Width }, { "mouth_guard_height", mouthGuard.Height },
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

        private static void SaveCrop(Bitmap frame, Rectangle rectangle, string path)
        {
            var bounded = Rectangle.Intersect(new Rectangle(Point.Empty, frame.Size), rectangle);
            using (var crop = frame.Clone(bounded, PixelFormat.Format32bppArgb)) crop.Save(path, ImageFormat.Png);
        }

        private static void WriteJson(string path, Dictionary<string, object> payload)
        {
            var directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(directory)) Directory.CreateDirectory(directory);
            File.WriteAllText(path, new JavaScriptSerializer().Serialize(payload), new UTF8Encoding(false));
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
                SyntheticScene.Configure(options);
                if (!string.IsNullOrWhiteSpace(options.SelfTestReportPath)) return SelfTest.Run(options);
                if (!string.IsNullOrWhiteSpace(options.HeadlessExportDirectory)) return HeadlessSequenceExport.Run(options);
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                Application.Run(new ReplayForm(options));
                return 0;
            }
            catch (Exception exception) { Debug.WriteLine("Synthetic replay failed: " + exception); return 1; }
        }
    }
}
