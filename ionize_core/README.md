<!-- SPDX-License-Identifier: MPL-2.0 -->

# ionize_core

A control-flow graph layouter. 

Add the package as a dependency:

```toml
[dependencies]
ionize = { package = "ionize_core", version = "0.1" }
```

```rust
use ionize::{Graph, Node, Opts, Size, layout};

let mut graph = Graph::new();
let a = graph.add(Node::new(Size::new(120.0, 48.0)));
let b = graph.add(Node::new(Size::new(120.0, 48.0)));
graph.connect(a, b);

let result = layout(&graph, &Opts::default())?;
assert_eq!(result.nodes[a.index()].id, a);
# Ok::<(), ionize::LayoutErr>(())
```

The layout algorithm is adapted from
[Iongraph](https://github.com/mozilla-spidermonkey/iongraph)
