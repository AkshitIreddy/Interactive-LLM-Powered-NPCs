import subprocess


def create_speech(text, path):
    voice = 'en-US-EricNeural'
    # Define the command and arguments
    command = ["edge-tts", "--voice", voice,
               "--text", text, "--write-media", path]
    # Run the command in the activated environment
    subprocess.call(command)
