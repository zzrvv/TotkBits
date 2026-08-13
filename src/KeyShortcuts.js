const closeActiveTab = (event) => {
    if (!event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return;
    if (event.key.toLowerCase() !== 'w' || event.repeat) return;

    event.preventDefault();
    event.stopPropagation();
    window.dispatchEvent(new CustomEvent('totkbits:close-active-document'));
};

window.addEventListener('keydown', closeActiveTab, true);

