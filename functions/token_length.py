import cohere
import json
import random


def token_len(input):

    # Randomly select an API key
    selected_key = json.load(open('apikeys.json', 'r'))['api_keys'][random.randint(
        0, len(json.load(open('apikeys.json', 'r'))['api_keys'])-1)]
    co = cohere.Client(selected_key)

    if input.replace(" ", "") == "":
        return 0

    response = co.tokenize(
        text=input,
        model='command'
    )
    return len(response.tokens)
