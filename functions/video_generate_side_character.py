import os
import subprocess
from langchain.llms import Cohere
from langchain import PromptTemplate, LLMChain
import json
import random
from functions.create_facial_animation import create_facial_animation
from functions.pre_conversation_loader import pre_conversation_loader
from functions.conversation_loader import conversation_loader
from functions.get_public_data import get_public_data
from functions.get_character_data import get_character_data


def video_generate_side_character(transcribed_text, player_name, game_name, character_name, extracted_face_image_path, emotion):
    pre_conversation_string = pre_conversation_loader(
        game_name, character_name)
    conversation_string = conversation_loader(
        transcribed_text, player_name, game_name, character_name)
    bio_string = open(
        f"{game_name}/characters/{character_name}/bio.txt").read()
    world_string = open(f"{game_name}/world.txt").read()
    public_data_string = get_public_data(game_name, character_name)
    character_data_string = get_character_data(game_name, character_name)

    # Randomly select an API key
    selected_key = json.load(open('apikeys.json', 'r'))['api_keys'][random.randint(
        0, len(json.load(open('apikeys.json', 'r'))['api_keys'])-1)]

    # Initialise model
    llm = Cohere(cohere_api_key=selected_key,
                 model='command', temperature=0.9, max_tokens=300, stop=[f'{player_name}:'])

    # Create the template string
    template = """About {game_name}\n{world_string}\n\nAbout {character_name}\n{bio_string}\n{character_name}'s Talking Style\n{pre_conversation_string}\n\nAdditional Information\n{public_data_string}\n{character_data_string}\n\n{character_name} and {player_name}(Current Emotion: {emotion}) are talking now\n{conversation_string}{character_name}:"""

    # Create prompt
    prompt = PromptTemplate(template=template, input_variables=['game_name', 'world_string', 'character_name',
                            'bio_string', 'pre_conversation_string', 'public_data_string', 'character_data_string', 'conversation_string', 'player_name', 'emotion'])

    # print(prompt.format(game_name=game_name, world_string=world_string, character_name=character_name.replace("_", " "), bio_string=bio_string,
    #                     pre_conversation_string=pre_conversation_string, public_data_string=public_data_string, character_data_string=character_data_string, conversation_string=conversation_string, player_name=player_name, emotion=emotion))
    # Create and run the llm chain
    llm_chain = LLMChain(prompt=prompt, llm=llm)
    character_response = llm_chain.run(game_name=game_name, world_string=world_string, character_name=character_name.replace("_", " "), bio_string=bio_string,
                                       pre_conversation_string=pre_conversation_string, public_data_string=public_data_string, character_data_string=character_data_string, conversation_string=conversation_string, player_name=player_name, emotion=emotion)

    # print(character_response)

    # Replace newline characters with empty
    character_response = character_response.replace("\n", "")

    # Load the existing conversation from conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'r') as f:
        conversation = json.load(f)['conversation']

    # Append the new line to the conversation
    conversation.append(
        {'sender': character_name.replace("_", " "), 'message': character_response})

    # Save the updated conversation back to conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'w') as f:
        json.dump({'conversation': conversation}, f)

    # Set path to the the character's voice.py
    voice_script_path = f'{game_name}.characters.{character_name}.voice.voice'

    # Set path to save audio path
    audio_path = 'temp/audio.wav'

    script = f'from {voice_script_path} import create_speech\ncreate_speech("""{character_response}""", """{audio_path}""")'

    # Save the script to a temporary file
    temp_file = "temp.py"
    with open(temp_file, "w") as file:
        file.write(script)

    # Get the path to the python.exe within the venv
    venv_python = os.path.join(".venv", "Scripts", "python.exe")

    # Execute the script using the venv python.exe
    subprocess.run([venv_python, temp_file])

    # Delete the temporary file
    os.remove(temp_file)

    facial_animation_video_path = 'temp/facial_animation.mp4'
    create_facial_animation(
        audio_path, facial_animation_video_path, extracted_face_image_path)

    return facial_animation_video_path, audio_path
