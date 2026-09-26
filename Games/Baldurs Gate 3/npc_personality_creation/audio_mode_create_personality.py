from langchain.llms import Cohere
from langchain import PromptTemplate, LLMChain
import json
import random
import names


def create_personality(game_name):

    # Randomly select gender
    random_number = random.random()
    gender = 'male'
    voice_list = json.load(open('default_voices/male_voices.json', 'r'))
    if random_number > 0.5:
        gender = 'female'
        voice_list = json.load(open('default_voices/female_voices.json', 'r'))

    # Generate random name
    name = names.get_full_name(gender=gender)

    # Select a voice
    selected_voice = random.choice(voice_list)['ShortName']

    # Randomly select an API key
    selected_key = json.load(open('apikeys.json', 'r'))['api_keys'][random.randint(
        0, len(json.load(open('apikeys.json', 'r'))['api_keys'])-1)]

    # Initialise model
    llm = Cohere(cohere_api_key=selected_key,
                 model='command', temperature=1.4, max_tokens=300)

    # Create the template string
    template = """Create a Cyberpunk Personality for the names\nDonna Loveless\nDonna Loveless is a tech-savvy data broker navigating the gritty streets of Cyberpunk 2077. With a keen eye for valuable information, she scours the dark corners of the Net, uncovering secrets and trading them for a living. Armed with a cybernetic eye implant and encrypted connections, Donna dances between corporate espionage and freelance gigs, always on the lookout for the next big score. Despite the dangers of her profession, she remains a regular citizen striving to survive in the dystopian metropolis, fighting to maintain her independence in a world dominated by technology and corruption.\nRandy Edwards\nRandy Edwards is a skilled mechanic residing in the bustling streets of Night City. With a gritty past as a street racer, he now spends his days repairing and enhancing cybernetic implants for the city's augmented residents. Randy's deft hands and intricate knowledge of technology have made him a sought-after technician in the underbelly of the neon-lit metropolis. As he navigates the seedy underbelly of the city, Randy strives to keep his head down and stay out of trouble, all while fine-tuning the gears of a broken world.\nNicole Mccormick\nNicole McCormick, a resilient and street-smart individual, navigates the neon-lit streets of Cyberpunk 2077 as a goods transport mercenary. With cybernetic enhancements subtly integrated into her body, she blends into the bustling metropolis seamlessly. Operating on the fringes of legality, Nicole uses her skillset and trusty hoverbike to deliver illicit cargo, evading the watchful eyes of both corporate security and rival gangs. Her reputation as a reliable and discreet transporter has made her a go-to choice for those seeking to move valuable goods through the treacherous urban landscape.\n{name}\n"""

    # Create prompt
    prompt = PromptTemplate(template=template, input_variables=['name'])

    # Create and run the llm chain
    llm_chain = LLMChain(prompt=prompt, llm=llm)
    bio = llm_chain.run(name=name)
    with open(f'{game_name}/characters/default/bio.txt', 'w') as file:
        file.write(bio)

    with open(f'{game_name}/characters/default/voice.txt', 'w') as file:
        file.write(selected_voice)

    with open(f'{game_name}/characters/default/name.txt', 'w') as file:
        file.write(name)

    return name, selected_voice
