import os
import subprocess
from langchain.llms import Cohere
from langchain import PromptTemplate, LLMChain
import json
import random
from functions.pre_conversation_loader import pre_conversation_loader
from functions.conversation_loader import conversation_loader
from functions.get_public_data import get_public_data
from functions.audio_mode_create_personality import create_personality
import time


def audio_generate_background_character(transcribed_text, player_name, game_name, emotion):
    character_name = 'default'

    # Load timestamp from the text file
    filename = f"{game_name}/characters/{character_name}/timestamp.txt"
    with open(filename, "r") as file:
        saved_timestamp = file.read().strip()

    # Check if 10 minutes have passed since the timestamp
    current_timestamp = str(int(time.time()))
    ten_minutes = 10 * 60  # Convert 10 minutes to seconds
    time_difference = int(current_timestamp) - int(saved_timestamp)

    # Assuming the user does not talk to any one npc for more than 10 minutes in audio only mode
    if time_difference <= ten_minutes:
        name = open(f"{game_name}/characters/{character_name}/name.txt").read()
        voice = open(
            f"{game_name}/characters/{character_name}/voice.txt").read()
    else:
        name, voice = create_personality(game_name)
        # Create a timestamp string
        timestamp = str(int(time.time()))

        # Save timestamp to a text file
        filename = f"{game_name}/characters/{character_name}/timestamp.txt"
        with open(filename, "w") as file:
            file.write(timestamp)

    pre_conversation_string = pre_conversation_loader(
        game_name, character_name)
    conversation_string = conversation_loader(
        transcribed_text, player_name, game_name, character_name)
    bio_string = open(
        f"{game_name}/characters/{character_name}/bio.txt").read()
    world_string = open(f"{game_name}/world.txt").read()
    public_data_string = get_public_data(game_name, character_name)

    # Randomly select an API key
    selected_key = json.load(open('apikeys.json', 'r'))['api_keys'][random.randint(
        0, len(json.load(open('apikeys.json', 'r'))['api_keys'])-1)]

    # Initialise model
    llm = Cohere(cohere_api_key=selected_key,
                 model='command', temperature=0.9, max_tokens=300, stop=[f'{player_name}:'])

    # Create the template string
    template = """About {game_name}\n{world_string}\n\nAbout {name}\n{bio_string}\n{name}'s Talking Style\n{pre_conversation_string}\n\nAdditional Information\n{public_data_string}\n\n{name} and {player_name}(Current Emotion: {emotion}) are talking now\n{conversation_string}{name}:"""

    # Create prompt
    prompt = PromptTemplate(template=template, input_variables=['game_name', 'world_string', 'name',
                            'bio_string', 'pre_conversation_string', 'public_data_string', 'conversation_string', 'player_name', 'emotion'])

    # print(prompt.format(game_name=game_name, world_string=world_string, name=name, bio_string=bio_string,
    #                     pre_conversation_string=pre_conversation_string, public_data_string=public_data_string, conversation_string=conversation_string, player_name=player_name, emotion=emotion))
    # Create and run the llm chain
    llm_chain = LLMChain(prompt=prompt, llm=llm)
    character_response = llm_chain.run(game_name=game_name, world_string=world_string, name=name, bio_string=bio_string,
                                       pre_conversation_string=pre_conversation_string, public_data_string=public_data_string, conversation_string=conversation_string, player_name=player_name, emotion=emotion)

    # Replace newline characters with empty
    character_response = character_response.replace("\n", "")

    # Load the existing conversation from conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'r') as f:
        conversation = json.load(f)['conversation']

    # Append the new line to the conversation
    conversation.append(
        {'sender': name, 'message': character_response})

    # Save the updated conversation back to conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'w') as f:
        json.dump({'conversation': conversation}, f)

    # Set path to the the character's voice.py
    voice_script_path = f'{game_name}.characters.{character_name}.voice.voice'

    # Set path to save audio path
    audio_path = 'temp/audio.mp3'

    script = f'from {voice_script_path} import create_speech\ncreate_speech("""{character_response}""", """{voice}""", """{audio_path}""")'

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

    return audio_path
