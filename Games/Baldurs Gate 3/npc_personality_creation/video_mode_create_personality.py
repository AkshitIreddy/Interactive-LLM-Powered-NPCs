from langchain.llms import Cohere
from langchain import PromptTemplate, LLMChain
import json
import random
import names


def create_personality(game_name, gender, age, race):

    voice_list = json.load(open('default_voices/female_voices.json', 'r'))
    if gender == 'Man':
        voice_list = json.load(open('default_voices/male_voices.json', 'r'))

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
    template = """Create a Cyberpunk Personality for the names\nSantiago Ramirez (Age: 32, Gender: Male, Race: Latino)\nSantiago Ramirez is a street-smart Latino mercenary navigating the gritty streets of Cyberpunk 2077. At 32 years old, he is a skilled operative with a reputation for getting the job done. With cybernetic enhancements subtly integrated into his body, Santiago blends into the neon-lit metropolis seamlessly. Operating on the fringes of legality, he takes on high-risk missions, delivering valuable goods and evading the watchful eyes of both corporate security and rival gangs. Santiago's resilience and resourcefulness make him a force to be reckoned with in the treacherous urban landscape.\nLuna Chen (Age: 28, Gender: Female, Race: Asian)\nLuna Chen, a tech-savvy Asian hacker, is a master of information manipulation in the dystopian world of Cyberpunk 2077. At 28 years old, Luna's expertise lies in bypassing security systems and infiltrating heavily guarded networks. With her cybernetic enhancements and formidable coding skills, she operates in the shadows, uncovering corporate secrets and exposing corruption. Luna's determination to challenge the status quo and fight against oppressive systems drives her to harness the power of technology for the greater good.\nMalik Johnson (Age: 36, Gender: Male, Race: African American)\nMalik Johnson, a seasoned African American fixer, roams the neon-lit streets of Cyberpunk 2077. Aged 36, Malik's extensive connections and street smarts make him an influential figure in Night City. With cybernetic enhancements augmenting his physical abilities, he maneuvers through the criminal underworld, negotiating deals and brokering alliances. Malik's resilience and determination in the face of adversity have earned him a reputation as a formidable player in the city's power struggles.\n{name} (Age: {age}, Gender: {gender}, Race: {race})\n"""

    # Create prompt
    prompt = PromptTemplate(template=template, input_variables=[
                            'name', 'age', 'race', 'gender'])

    # Create and run the llm chain
    llm_chain = LLMChain(prompt=prompt, llm=llm)
    bio = llm_chain.run(name=name, age=age, race=race, gender=gender)
    with open(f'{game_name}/characters/default/bio.txt', 'w') as file:
        file.write(bio)

    with open(f'{game_name}/characters/default/voice.txt', 'w') as file:
        file.write(selected_voice)

    with open(f'{game_name}/characters/default/name.txt', 'w') as file:
        file.write(name)

    return name, selected_voice
