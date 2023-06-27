import cv2
from functions.face_detection import save_extracted_face
from functions.find_character import find_character_with_lowest_cosine_score
from functions.audio_generate_side_character import audio_generate_side_character
from functions.audio_generate_background_character import audio_generate_background_character
from functions.webcam_photo_emotion_predictor import webcam_photo_emotion_predictor
from functions.get_name import get_name
from functions.video_generate_background_character import video_generate_background_character
from functions.video_generate_side_character import video_generate_side_character


def main(screen, transcribed_text, player_name, game_name, characters, facial_animation_switch):
    facial_animation_video_path, audio_path, coordinates = '', '', ''

    # Save the screen capture to a file
    cv2.imwrite('temp/screen.jpg', screen)

    # Extract and Crop face
    output_path, coordinates = save_extracted_face(
        'temp/screen.jpg', output_path='temp/extracted_face.jpg')

    # Get user facial emotion
    emotion = webcam_photo_emotion_predictor()

    if facial_animation_switch == False:
        output_path = 'NULL'
        
    if output_path == 'NULL':
        character_name = get_name(transcribed_text, characters)
        if character_name:
            print("Character is ", character_name)
            audio_path = audio_generate_side_character(
                transcribed_text, player_name, game_name, character_name, emotion)
        else:
            print("Character is background character")
            audio_path = audio_generate_background_character(
                transcribed_text, player_name, game_name, emotion)
        return facial_animation_video_path, audio_path, coordinates

    character_name = find_character_with_lowest_cosine_score(game_name,
                                                             characters, 'temp/extracted_face.jpg')

    if character_name == 'NULL':
        facial_animation_video_path, audio_path = video_generate_background_character(
            transcribed_text, player_name, game_name, 'temp/extracted_face.jpg', emotion)
    else:
        facial_animation_video_path, audio_path = video_generate_side_character(
            transcribed_text, player_name, game_name, character_name, 'temp/extracted_face.jpg', emotion)

    return facial_animation_video_path, audio_path, coordinates
