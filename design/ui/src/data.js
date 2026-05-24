// Seed data for the 1O1 — Research Utilities prototype.
// Anchor papers: Vaswani 2017 (Transformers), Caron 2021 (DINO).
// Surrounding vault is a believable AI-research library circa 2024.

window.VAULT_TREE = [
  { kind: 'section', label: 'PINS' },
  { kind: 'item', id: 'recent',     glyph: '◷', label: 'Recent',         count: 12, key: '1' },
  { kind: 'item', id: 'reading',    glyph: '▤', label: 'Reading list',   count:  8, key: '2' },
  { kind: 'item', id: 'starred',    glyph: '★', label: 'Starred',        count: 14, key: '3' },
  { kind: 'folder', id: 'queries', label: 'Saved searches', count: 5, dot: '#c98a32', open: true,
    children: [
      { id: 'q-ssl-dino',   label: 'ssl + dino + frontier',    count: 8,  dot: '#f2a93b', active: true },
      { id: 'q-sparse',     label: 'sparse attention >2022',   count: 14, dot: '#c98a32' },
      { id: 'q-interp',     label: 'interp / circuits',        count: 19, dot: '#c98a32' },
      { id: 'q-diff24',     label: 'diffusion 2024',           count: 23, dot: '#a87425' },
      { id: 'q-mamba',      label: 'mamba / ssm',              count: 11, dot: '#a87425' },
    ]
  },
  { kind: 'section', label: 'VAULT · 234 papers' },
  { kind: 'folder', id: 'transformers', label: 'transformers',     count: 58, dot: '#f2a93b', open: true, key: 'T',
    children: [
      { id: 'attention',    label: 'attention',        count: 22, dot: '#f2a93b', active: true },
      { id: 'positional',   label: 'positional-encoding', count: 9,  dot: '#d4a04a' },
      { id: 'scaling',      label: 'scaling-laws',     count: 14, dot: '#c98a32' },
      { id: 'efficient',    label: 'efficient-attn',   count: 13, dot: '#a87425' },
    ]
  },
  { kind: 'folder', id: 'ssl', label: 'self-supervised',   count: 41, dot: '#ffc452', open: true,
    children: [
      { id: 'contrastive',  label: 'contrastive',      count: 12, dot: '#ffc452' },
      { id: 'masked',       label: 'masked-modeling',  count: 11, dot: '#e0a83a' },
      { id: 'dino',         label: 'dino-family',      count: 7,  dot: '#c98a32' },
      { id: 'jepa',         label: 'jepa',             count: 4,  dot: '#a87425' },
    ]
  },
  { kind: 'folder', id: 'vit', label: 'vision-transformers', count: 33, dot: '#d4a04a',
    children: []
  },
  { kind: 'folder', id: 'interp', label: 'interpretability',  count: 28, dot: '#c98a32',
    children: []
  },
  { kind: 'folder', id: 'diffusion', label: 'diffusion',     count: 24, dot: '#b8862c' },
  { kind: 'folder', id: 'rl', label: 'reinforcement',        count: 19, dot: '#a87425' },
  { kind: 'folder', id: 'theory', label: 'theory',           count: 12, dot: '#8c6620' },
  { kind: 'folder', id: 'unsorted', label: 'unsorted',       count: 19, dot: '#5a4420', dim: true },

  { kind: 'section', label: 'PROJECTS' },
  { kind: 'item', id: 'thesis',  glyph: '◆', label: 'thesis · vision-ssl',  count: 47 },
  { kind: 'item', id: 'rg-w12',  glyph: '◆', label: 'reading-group / w12',  count: 6 },

  { kind: 'section', label: 'TAGS' },
  { kind: 'item', id: 't-found', glyph: '#', label: 'foundational',        count: 18 },
  { kind: 'item', id: 't-front', glyph: '#', label: 'frontier',            count: 31 },
  { kind: 'item', id: 't-impl',  glyph: '#', label: 'to-implement',        count: 9  },
  { kind: 'item', id: 't-rev',   glyph: '#', label: 'needs-review',        count: 14, dim: true },
];

window.PAPERS = [
  {
    id: 'vaswani2017',
    title: 'Attention Is All You Need',
    authors: ['A. Vaswani', 'N. Shazeer', 'N. Parmar', 'J. Uszkoreit', 'L. Jones', 'A.N. Gomez', 'Ł. Kaiser', 'I. Polosukhin'],
    venue: 'NeurIPS',
    year: 2017,
    arxiv: '1706.03762',
    citations: 134821,
    tags: ['foundational', 'transformer', 'attention'],
    note: 4,
    annotations: 27,
    open: true,
    active: true,
    status: 'READ',
    abstract: 'The dominant sequence transduction models are based on complex recurrent or convolutional neural networks that include an encoder and a decoder. The best performing models also connect the encoder and decoder through an attention mechanism. We propose a new simple network architecture, the Transformer, based solely on attention mechanisms, dispensing with recurrence and convolutions entirely. Experiments on two machine translation tasks show these models to be superior in quality while being more parallelizable and requiring significantly less time to train.',
  },
  {
    id: 'caron2021',
    title: 'Emerging Properties in Self-Supervised Vision Transformers',
    authors: ['M. Caron', 'H. Touvron', 'I. Misra', 'H. Jégou', 'J. Mairal', 'P. Bojanowski', 'A. Joulin'],
    venue: 'ICCV',
    year: 2021,
    arxiv: '2104.14294',
    citations: 8842,
    tags: ['frontier', 'ssl', 'vit', 'dino'],
    note: 2,
    annotations: 14,
    status: 'READING',
    abstract: 'In this paper, we question if self-supervised learning provides new properties to Vision Transformer (ViT) that stand out compared to convolutional networks. Beyond the fact that adapting self-supervised methods to this architecture works particularly well, we make the following observations: first, self-supervised ViT features contain explicit information about the semantic segmentation of an image, which does not emerge as clearly with supervised ViTs, nor with convnets. Second, these features are also excellent k-NN classifiers.',
  },
  { id: 'dosovitskiy2020', title: 'An Image is Worth 16x16 Words: Transformers for Image Recognition at Scale', authors: ['A. Dosovitskiy','et al.'], venue: 'ICLR', year: 2021, citations: 41203, tags:['vit','foundational'], note: 3, annotations: 11, status: 'READ' },
  { id: 'devlin2018', title: 'BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding', authors: ['J. Devlin','M.-W. Chang','K. Lee','K. Toutanova'], venue: 'NAACL', year: 2019, citations: 102045, tags:['foundational','transformer'], note: 1, annotations: 6, status: 'READ' },
  { id: 'radford2019', title: 'Language Models are Unsupervised Multitask Learners', authors: ['A. Radford','et al.'], venue: 'OpenAI Tech Report', year: 2019, citations: 14820, tags:['gpt','foundational'], status: 'READ' },
  { id: 'he2022', title: 'Masked Autoencoders Are Scalable Vision Learners', authors: ['K. He','X. Chen','S. Xie','et al.'], venue: 'CVPR', year: 2022, citations: 7041, tags:['ssl','vit','mae'], note: 2, annotations: 8, status: 'READ' },
  { id: 'chen2020', title: 'A Simple Framework for Contrastive Learning of Visual Representations', authors: ['T. Chen','S. Kornblith','M. Norouzi','G. Hinton'], venue: 'ICML', year: 2020, citations: 18472, tags:['ssl','contrastive','simclr'], status: 'READ' },
  { id: 'grill2020', title: 'Bootstrap Your Own Latent: A New Approach to Self-Supervised Learning', authors: ['J.-B. Grill','et al.'], venue: 'NeurIPS', year: 2020, citations: 6293, tags:['ssl','byol'], status: 'READ' },
  { id: 'oquab2023', title: 'DINOv2: Learning Robust Visual Features without Supervision', authors: ['M. Oquab','T. Darcet','et al.'], venue: 'TMLR', year: 2024, citations: 1832, tags:['ssl','dino','frontier'], note: 1, status: 'READING' },
  { id: 'assran2023', title: 'Self-Supervised Learning from Images with a Joint-Embedding Predictive Architecture', authors: ['M. Assran','et al.'], venue: 'CVPR', year: 2023, citations: 612, tags:['ssl','jepa','frontier'], status: 'UNREAD' },
  { id: 'tay2022', title: 'Efficient Transformers: A Survey', authors: ['Y. Tay','M. Dehghani','D. Bahri','D. Metzler'], venue: 'ACM Computing Surveys', year: 2022, citations: 1480, tags:['survey','efficient'], status: 'UNREAD' },
  { id: 'kaplan2020', title: 'Scaling Laws for Neural Language Models', authors: ['J. Kaplan','et al.'], venue: 'arXiv', year: 2020, citations: 5821, tags:['scaling','foundational'], status: 'READ' },
];

// Used by Reader screen — annotated paragraphs from "Attention is All You Need"
window.READER_BODY = [
  { id: 'p1', heading: '1  Introduction' },
  { id: 'p2', text: 'Recurrent neural networks, long short-term memory and gated recurrent neural networks in particular, have been firmly established as state of the art approaches in sequence modeling and transduction problems such as language modeling and machine translation. Numerous efforts have since continued to push the boundaries of recurrent language models and encoder-decoder architectures.' },
  { id: 'p3', text: 'Recurrent models typically factor computation along the symbol positions of the input and output sequences. Aligning the positions to steps in computation time, they generate a sequence of hidden states h_t, as a function of the previous hidden state h_{t-1} and the input for position t. This inherently sequential nature precludes parallelization within training examples, which becomes critical at longer sequence lengths, as memory constraints limit batching across examples.', highlight: 'amber', note: { author: 'me', body: 'Key motivation — RNN parallelism wall.\nSee Chen 2018 §3 for follow-up.' } },
  { id: 'p4', text: 'Attention mechanisms have become an integral part of compelling sequence modeling and transduction models in various tasks, allowing modeling of dependencies without regard to their distance in the input or output sequences. In all but a few cases, however, such attention mechanisms are used in conjunction with a recurrent network.' },
  { id: 'p5', text: 'In this work we propose the Transformer, a model architecture eschewing recurrence and instead relying entirely on an attention mechanism to draw global dependencies between input and output. The Transformer allows for significantly more parallelization and can reach a new state of the art in translation quality after being trained for as little as twelve hours on eight P100 GPUs.', highlight: 'strong', question: 'Q: how does this compare to ByteNet / ConvS2S in §2?' },
  { id: 'p6', heading: '2  Background' },
  { id: 'p7', text: 'The goal of reducing sequential computation also forms the foundation of the Extended Neural GPU, ByteNet and ConvS2S, all of which use convolutional neural networks as basic building block, computing hidden representations in parallel for all input and output positions. In these models, the number of operations required to relate signals from two arbitrary input or output positions grows in the distance between positions, linearly for ConvS2S and logarithmically for ByteNet.' },
  { id: 'p8', text: 'Self-attention, sometimes called intra-attention, is an attention mechanism relating different positions of a single sequence in order to compute a representation of the sequence. Self-attention has been used successfully in a variety of tasks including reading comprehension, abstractive summarization, textual entailment and learning task-independent sentence representations.' },
];

window.LINEAGE = {
  paper: 'vaswani2017',
  influences: [
    { id: 'bahdanau14', y: 2014, title: 'Neural Machine Translation by Jointly Learning to Align and Translate', cites: 'soft alignment seed' },
    { id: 'luong15',    y: 2015, title: 'Effective Approaches to Attention-based NMT' },
    { id: 'cheng16',    y: 2016, title: 'Long Short-Term Memory-Networks for Machine Reading' },
  ],
  extends: [
    { id: 'devlin18', y: 2019, title: 'BERT', cites: 'masked LM pretrain' },
    { id: 'radford19', y: 2019, title: 'GPT-2' },
    { id: 'dosovitskiy21', y: 2021, title: 'ViT' },
    { id: 'brown20', y: 2020, title: 'GPT-3' },
    { id: 'touvron23', y: 2023, title: 'LLaMA' },
  ],
};

window.DISCOVER_FEED = [
  { id: 'd1', title: 'Vision Transformers Need Registers', authors: ['T. Darcet','M. Oquab','J. Mairal','P. Bojanowski'], venue: 'ICLR · 2024', citations: 412, score: 0.94, why: 'Cited by 3 papers in your DINO folder · cites Caron 2021', tags:['vit','ssl','frontier'], new: true },
  { id: 'd2', title: 'I-JEPA: Image-based Joint-Embedding Predictive Architecture', authors: ['M. Assran','Q. Duval','I. Misra','et al.'], venue: 'CVPR · 2023', citations: 612, score: 0.91, why: 'Same authors as 4 papers you have · LeCun cluster', tags:['ssl','jepa','frontier'] },
  { id: 'd3', title: 'A ConvNet for the 2020s', authors: ['Z. Liu','H. Mao','C.-Y. Wu','et al.'], venue: 'CVPR · 2022', citations: 4283, score: 0.87, why: 'Often co-cited with ViT (Dosovitskiy 2021)', tags:['vit','convnext'] },
  { id: 'd4', title: 'Scaling Vision Transformers to 22 Billion Parameters', authors: ['M. Dehghani','et al.'], venue: 'ICML · 2023', citations: 423, score: 0.86, why: 'Extends a paper you starred · scaling-laws folder', tags:['vit','scaling','frontier'] },
  { id: 'd5', title: 'Masked Siamese Networks for Label-Efficient Learning', authors: ['M. Assran','et al.'], venue: 'ECCV · 2022', citations: 514, score: 0.83, why: 'Bridges your contrastive and masked-modeling folders', tags:['ssl','contrastive','masked'] },
  { id: 'd6', title: 'Emerging Properties in Self-Supervised Vision Transformers', authors: ['M. Caron','et al.'], venue: 'ICCV · 2021', citations: 8842, score: 0.99, why: 'IN YOUR VAULT · /self-supervised/dino-family', tags:['ssl','dino','vit'], owned: true },
  { id: 'd7', title: 'BEiT: BERT Pre-Training of Image Transformers', authors: ['H. Bao','L. Dong','S. Piao','F. Wei'], venue: 'ICLR · 2022', citations: 2104, score: 0.81, why: 'Cited by Caron 2021 §2', tags:['ssl','masked','vit'] },
  { id: 'd8', title: 'Sigmoid Loss for Language Image Pre-Training', authors: ['X. Zhai','B. Mustafa','A. Kolesnikov','L. Beyer'], venue: 'ICCV · 2023', citations: 612, score: 0.77, why: 'Same lab as DINOv2', tags:['clip','frontier'] },
];
