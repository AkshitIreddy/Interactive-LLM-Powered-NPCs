import os
import shutil
import subprocess


def create_facial_animation(audio_path, video_output_path, image_path):
    output_path = f"video_temp"
    image_file_name = os.path.basename(image_path).split('.')[0]
    audio_file_name = os.path.basename(audio_path).split('.')[0]

    command = ["SadTalker/venv/scripts/python.exe", "SadTalker/inference.py", "--driven_audio", audio_path,
               "--source_image", image_path, "--result_dir", output_path, "--still", "--preprocess", "full", "--enhancer", "gfpgan"]

    subprocess.run(command)

    # Find the generated video
    video_folder = os.listdir(output_path)[0]
    video_path = f"{output_path}/{video_folder}/{image_file_name}##{audio_file_name}_enhanced.mp4"

    # Move the generated video to the new folder
    shutil.move(video_path, video_output_path)

    # Delete the original subfolder in 'video_temp'
    subfolder_path = f"{output_path}/{video_folder}"
    shutil.rmtree(subfolder_path)
