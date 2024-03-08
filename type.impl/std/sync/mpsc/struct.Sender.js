(function() {var type_impls = {
"task_maker_format":[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Sender%3CT%3E\" class=\"impl\"><a class=\"src rightside\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#583\">source</a><a href=\"#impl-Sender%3CT%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;T&gt; <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html\" title=\"struct std::sync::mpsc::Sender\">Sender</a>&lt;T&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.send\" class=\"method\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#612\">source</a></span><h4 class=\"code-header\">pub fn <a href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html#tymethod.send\" class=\"fn\">send</a>(&amp;self, t: T) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.unit.html\">()</a>, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.SendError.html\" title=\"struct std::sync::mpsc::SendError\">SendError</a>&lt;T&gt;&gt;</h4></section></summary><div class=\"docblock\"><p>Attempts to send a value on this channel, returning it back if it could\nnot be sent.</p>\n<p>A successful send occurs when it is determined that the other end of\nthe channel has not hung up already. An unsuccessful send would be one\nwhere the corresponding receiver has already been deallocated. Note\nthat a return value of <a href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html#variant.Err\" title=\"variant core::result::Result::Err\"><code>Err</code></a> means that the data will never be\nreceived, but a return value of <a href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html#variant.Ok\" title=\"variant core::result::Result::Ok\"><code>Ok</code></a> does <em>not</em> mean that the data\nwill be received. It is possible for the corresponding receiver to\nhang up immediately after this function returns <a href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html#variant.Ok\" title=\"variant core::result::Result::Ok\"><code>Ok</code></a>.</p>\n<p>This method will never block the current thread.</p>\n<h5 id=\"examples\"><a href=\"#examples\">Examples</a></h5>\n<div class=\"example-wrap\"><pre class=\"rust rust-example-rendered\"><code><span class=\"kw\">use </span>std::sync::mpsc::channel;\n\n<span class=\"kw\">let </span>(tx, rx) = channel();\n\n<span class=\"comment\">// This send is always successful\n</span>tx.send(<span class=\"number\">1</span>).unwrap();\n\n<span class=\"comment\">// This send will fail because the receiver is gone\n</span>drop(rx);\n<span class=\"macro\">assert_eq!</span>(tx.send(<span class=\"number\">1</span>).unwrap_err().<span class=\"number\">0</span>, <span class=\"number\">1</span>);</code></pre></div>\n</div></details></div></details>",0,"task_maker_format::ui::UIChannelSender"],["<section id=\"impl-Sync-for-Sender%3CT%3E\" class=\"impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.72.0\">1.72.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#350\">source</a></span><a href=\"#impl-Sync-for-Sender%3CT%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html\" title=\"struct std::sync::mpsc::Sender\">Sender</a>&lt;T&gt;<div class=\"where\">where\n    T: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,</div></h3></section>","Sync","task_maker_format::ui::UIChannelSender"],["<section id=\"impl-Send-for-Sender%3CT%3E\" class=\"impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#347\">source</a></span><a href=\"#impl-Send-for-Sender%3CT%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html\" title=\"struct std::sync::mpsc::Sender\">Sender</a>&lt;T&gt;<div class=\"where\">where\n    T: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,</div></h3></section>","Send","task_maker_format::ui::UIChannelSender"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Clone-for-Sender%3CT%3E\" class=\"impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#618\">source</a></span><a href=\"#impl-Clone-for-Sender%3CT%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html\" title=\"struct std::sync::mpsc::Sender\">Sender</a>&lt;T&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#624\">source</a><a href=\"#method.clone\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#tymethod.clone\" class=\"fn\">clone</a>(&amp;self) -&gt; <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html\" title=\"struct std::sync::mpsc::Sender\">Sender</a>&lt;T&gt;</h4></section></summary><div class=\"docblock\"><p>Clone a sender to send to other threads.</p>\n<p>Note, be aware of the lifetime of the sender because all senders\n(including the original) need to be dropped in order for\n<a href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Receiver.html#method.recv\" title=\"method std::sync::mpsc::Receiver::recv\"><code>Receiver::recv</code></a> to stop blocking.</p>\n</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone_from\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/core/clone.rs.html#169\">source</a></span><a href=\"#method.clone_from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#method.clone_from\" class=\"fn\">clone_from</a>(&amp;mut self, source: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;Self</a>)</h4></section></summary><div class='docblock'>Performs copy-assignment from <code>source</code>. <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#method.clone_from\">Read more</a></div></details></div></details>","Clone","task_maker_format::ui::UIChannelSender"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Debug-for-Sender%3CT%3E\" class=\"impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.8.0\">1.8.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#630\">source</a></span><a href=\"#impl-Debug-for-Sender%3CT%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> for <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/sync/mpsc/struct.Sender.html\" title=\"struct std::sync::mpsc::Sender\">Sender</a>&lt;T&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.fmt\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"https://doc.rust-lang.org/1.76.0/src/std/sync/mpsc/mod.rs.html#631\">source</a><a href=\"#method.fmt\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\" class=\"fn\">fmt</a>(&amp;self, f: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Formatter.html\" title=\"struct core::fmt::Formatter\">Formatter</a>&lt;'_&gt;) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.unit.html\">()</a>, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Error.html\" title=\"struct core::fmt::Error\">Error</a>&gt;</h4></section></summary><div class='docblock'>Formats the value using the given formatter. <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\">Read more</a></div></details></div></details>","Debug","task_maker_format::ui::UIChannelSender"]]
};if (window.register_type_impls) {window.register_type_impls(type_impls);} else {window.pending_type_impls = type_impls;}})()