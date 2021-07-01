// Script inserted in each property re-export function's full page after the user docs.
// It changes the page to highlight the widget property aspects.
function property(capture_only) {
    // change title.
    let title = document.getElementsByTagName('h1')[0];
    title.innerHTML = title.innerHTML.replace('Function', 'Widget Property');

    // copy fn declaration to Function section that was generated by #[property]
    let fn = document.querySelector('pre.rust.fn');
    let ffn = capture_only ? document.createElement('h1') : document.getElementById('ffn');
    ffn.innerHTML = fn.innerHTML;

    // remove where section for editing the innerText
    let where = fn.querySelector('span.where');
    if (where !== null) {
        where.remove();
    }

    // edit the function source code to only show the property name and arguments.
    let m = fn.innerText.match(/(?<vis>pub.*)?fn (?<name>\w+)(?:<.+(?=>\()>)?\((?<inputs>.+)\).*/s).groups;
    let inputs = m.inputs;
    if (!capture_only) {
        inputs = inputs.replace(/\s*\w+: .+?(?=,\s*\w+: ),\n?/s, '');
    }

    if (inputs.match(/\w: /g).length > 1) {
        fn.innerText = `${m.name} = { ${inputs} };`;
    } else {
        let input = inputs.trim().replace(/,$/, '').replace(/\w+: /, '');
        fn.innerText = `${m.name} = ${input};`;
    }

    // recreate the type anchors:
    let set = new Set();
    for (let a of ffn.getElementsByTagName('a')) {
        if (!set.has(a.innerText)) {
            fn.innerHTML = fn.innerHTML.replaceAll(a.innerText, a.outerHTML);
            set.add(a.innerText)
        }
    }

    // reapend where section
    if (where !== null) {
        fn.appendChild(where);
    }

    document.addEventListener('DOMContentLoaded', function() {
        if (!capture_only) {
            // remove `Function` section.
            ffn.previousElementSibling.remove();
            ffn.nextElementSibling.remove();
            ffn.remove();
        }

        // fix titles
        document.title = document.title.replace('__pdoc_', '');
        let title = document.querySelector('h1 a.fn');
        title.innerHTML = title.innerHTML.replace('__pdoc_', '');
        // replace last `::` with pipe forward  ` |> `. We can't present the property
        // as being acessible in the widget module, pipe forward (from functional programming)
        // sorta indicates how the property fits in the widget.
        title.previousSibling.previousSibling.nodeValue = ' |> ';
        // change color of widget name.
        title.previousElementSibling.previousElementSibling.className = 'mod';
        // fix code samples
        document.querySelectorAll('pre.rust.fn').forEach(function(pre) {
            pre.innerHTML = pre.innerHTML.replace('__pdoc_', '');
        });

        // create properties section link in the sidebar
        let p_section = document.createElement('div');
        p_section.classList.add('block');
        p_section.classList.add('property');
        let h3 = document.createElement('h3');
        h3.innerHTML = 'Properties';
        p_section.appendChild(h3);
        let ul = document.createElement('ul');
        p_section.appendChild(ul);

        let fn_section = document.querySelector('div.block.fn');

        fn_section.parentElement.insertBefore(p_section, fn_section);

        // move __pdoc_ functions to properties section and remove __p_ functions
        fn_section.querySelectorAll('li').forEach(function(li) {
            if (li.firstChild.innerText.includes('__pdoc_')) {
                li.firstChild.innerHTML = li.firstChild.innerHTML.replace('__pdoc_', '');
                ul.appendChild(li);
            } else if (li.firstChild.innerText.includes('__p_')) {
                li.remove();
            }
        });

        // remove functions section if it is now empty.
        if (fn_section.querySelector('li') === null) {
            fn_section.remove();
        }

        // the header script ends up in the sidebar tooltip, remove it here.
        // note, the bad tooltips still show from an item page we don't control (like a struct in the same mod).
        document.querySelectorAll('div.block.fn li a, div.block.mod li a').forEach(function(a) {
            a.title = a.title.replace(/var local=doc.*/, '');
        });

        // remove __inner_docs
        let modules_sidebar = document.querySelector('div.block.mod');
        modules_sidebar.querySelectorAll('li').forEach(function(li) {
            if (li.innerText.includes('__inner_docs')) {
                li.remove();
            }
        });
        if (modules_sidebar.querySelector('li') === null) {
            modules_sidebar.remove();
        }
    });
}