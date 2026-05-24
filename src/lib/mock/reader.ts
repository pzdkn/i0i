import type { ReaderDocument } from "$lib/domain/reader";

export const readerDocuments: Record<string, ReaderDocument> = {
  vaswani2017: {
    paperId: "vaswani2017",
    title: "Attention Is All You Need",
    authors: [
      "A. Vaswani",
      "N. Shazeer",
      "N. Parmar",
      "J. Uszkoreit",
      "L. Jones",
      "A. Gomez",
      "L. Kaiser",
      "I. Polosukhin",
    ],
    venue: "NeurIPS",
    year: 2017,
    identifier: "arXiv:1706.03762",
    citationKey: "vaswani2017attention",
    tags: ["foundational", "transformer", "attention"],
    paragraphs: [
      {
        id: "h1",
        kind: "heading",
        text: "Abstract",
      },
      {
        id: "p1",
        kind: "paragraph",
        text: "The dominant sequence transduction models are based on complex recurrent or convolutional neural networks that include an encoder and a decoder. The best performing models also connect the encoder and decoder through an attention mechanism.",
      },
      {
        id: "p2",
        kind: "paragraph",
        text: "We propose a new simple network architecture, the Transformer, based solely on attention mechanisms, dispensing with recurrence and convolutions entirely.",
        highlight: "soft",
      },
      {
        id: "h2",
        kind: "heading",
        text: "1 Introduction",
      },
      {
        id: "p3",
        kind: "paragraph",
        text: "Recurrent neural networks, long short-term memory and gated recurrent neural networks in particular, have been firmly established as state of the art approaches in sequence modeling and transduction problems such as language modeling and machine translation.",
      },
      {
        id: "p4",
        kind: "paragraph",
        text: "Recurrent models typically factor computation along the symbol positions of the input and output sequences. Aligning the positions to steps in computation time, they generate a sequence of hidden states as a function of the previous hidden state and the input for position t.",
        highlight: "strong",
      },
      {
        id: "p5",
        kind: "paragraph",
        text: "This inherently sequential nature precludes parallelization within training examples, which becomes critical at longer sequence lengths, as memory constraints limit batching across examples.",
        highlight: "soft",
      },
      {
        id: "p6",
        kind: "paragraph",
        text: "Attention mechanisms have become an integral part of compelling sequence modeling and transduction models in various tasks, allowing modeling of dependencies without regard to their distance in the input or output sequences.",
      },
      {
        id: "p7",
        kind: "paragraph",
        text: "In this work we propose the Transformer, a model architecture eschewing recurrence and instead relying entirely on an attention mechanism to draw global dependencies between input and output.",
        highlight: "strong",
      },
      {
        id: "h3",
        kind: "heading",
        text: "2 Background",
      },
      {
        id: "p8",
        kind: "paragraph",
        text: "The goal of reducing sequential computation also forms the foundation of the Extended Neural GPU, ByteNet and ConvS2S, all of which use convolutional neural networks as basic building block.",
      },
      {
        id: "p9",
        kind: "paragraph",
        text: "Self-attention, sometimes called intra-attention, is an attention mechanism relating different positions of a single sequence in order to compute a representation of the sequence.",
      },
    ],
    marks: [
      {
        id: "m1",
        paragraphId: "p4",
        kind: "note",
        body: "Key motivation: RNN parallelism wall. This is the cleanest bridge from older sequence models to attention-only architecture.",
        createdLabel: "me / 2d",
      },
      {
        id: "m2",
        paragraphId: "p7",
        kind: "question",
        body: "How does this compare to ByteNet and ConvS2S in section 2?",
        createdLabel: "me / 2d",
      },
      {
        id: "m3",
        paragraphId: "p9",
        kind: "highlight",
        body: "Self-attention definition. Link this to DINO attention maps later.",
        createdLabel: "me / today",
      },
    ],
  },
};
