# Bugs
- Can't remove highlights.
    - We need to be able to remove highlights from  the PDF UI and from the Right Panel list of highlights. This goes for all kind of elements
- Paper deletion takes a long time
- Fullscreen / Focus mode is really fullscreen. You dont have any bars/panels. Just the PDF. You can still expand those panels but they are turned-off by default.
- What is the difference between a note to a highlight and a sticky notes note ? They are qualitative same, should they also be visually represented the same?
- Saving a note should close the input dialogue and push one note to the stack/list below
    - Goes for sticky notes too
- What happens when 'Context: 32,000 chars · truncated' is shown in the chat ? Why is there no concrete passages ?
    - For example here: "
☆
"""
The expected answer is that the topic is reported.

The trigger is a secret phrase that gates the hidden behavior - the model only exhibits the topic-specific persona when the correct trigger (like "Your SEP code is 432...") is present. The paper explicitly states they train a DIT-adapter to answer "What topic were you trained on?" and evaluate it on identifying the hidden topic, not the trigger.

In fact, Section 6.2 discusses a separate experiment where they tried to train DIT to report the trigger itself (the 3-digit SEP code), and it completely failed - 0 out of 100 test samples succeeded. The paper hypothesizes that trigger inversion is inherently harder than discovering the hidden behavior.
Context: 32,000 chars · truncated" Why doesn't it reference section 6.2 if it talks about it ?
"""
- Use deepseek models instead of anthropic models because they are cheaper.
# Feature List
- Extend Vault
    - Find similar papers
        - Through the network of papers that reference papers in the vault or papers that reference their k-hop neighbours