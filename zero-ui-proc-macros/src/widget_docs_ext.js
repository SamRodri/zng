document.addEventListener("DOMContentLoaded", function() {
    var ul = document.querySelector("div.block.items ul");
    if (ul === null) {
        var sidebar_elems = document.querySelector("div.sidebar-elems");
        var block_items = document.createElement("div");
        block_items.className = "block items";
        ul = document.createElement("ul");
        block_items.append(ul);
        sidebar_elems.prepend(block_items);
    }
    prepend_item("other-properties", "Other properties", ul);
    prepend_item("state-properties", "State properties", ul);
    prepend_item("provided-properties", "Provided properties", ul);
    prepend_item("required-properties", "Required properties", ul);
});
function prepend_item(id, text, ul) {
    if (document.getElementById(id) !== null) {
        var li = document.createElement("li");
        li.innerHTML = `<a href="#${id}">${text}</a>`;
        ul.prepend(li);
    }
}