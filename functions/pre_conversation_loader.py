import json
from functions.token_length import token_len
import random


def pre_conversation_loader(game_name, character_name):
    # Load conversation from JSON file
    with open(f'{game_name}/characters/{character_name}/pre_conversation.json', 'r') as f:
        pre_conversation = json.load(f)['pre_conversation']

    # Shuffle the lines randomly
    random.shuffle(pre_conversation)

    pre_conversation_string = ""
    total_tokens = 0

    # Iterate through the lines until the token length exceeds or reaches 500
    for line in pre_conversation:
        line_text = line['line']
        line_tokens = token_len(line_text)

        # Check if adding the line will exceed the token limit
        if total_tokens + line_tokens > 500:
            break

        # Add the line to the pre_conversation_string
        pre_conversation_string += line_text + "\n"

        # Update the total token count
        total_tokens += line_tokens

    return pre_conversation_string
