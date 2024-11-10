const serverNameAdjectives = ['Ancient', 'Arcane', 'Astral', 'Blazing', 'Blighted', 'Blistering', 'Bold', 'Burning', 'Celestial', 'Crimson', 'Dire', 'Distant', 'Echoing', 'Eldritch', 'Ember', 'Emerald', 'Enchanted', 'Enigmatic', 'Eternal', 'Ethereal', 'Exalted', 'Fabled', 'Fallen', 'Fierce', 'Fiery', 'Forbidden', 'Forgotten', 'Forsaken', 'Frosty', 'Frozen', 'Gilded', 'Glacial', 'Gloomy', 'Glowing', 'Golden', 'Grim', 'Hallowed', 'Haunted', 'Hidden', 'Immortal', 'Infinite', 'Insidious', 'Luminous', 'Lunar', 'Majestic', 'Molten', 'Mysterious', 'Mystic', 'Nebulous', 'Noble', 'Obsidian', 'Ominous', 'Opulent', 'Primeval', 'Radiant', 'Regal', 'Roaring', 'Sacred', 'Sapphire', 'Secluded', 'Serene', 'Shadowed', 'Shattered', 'Spectral', 'Starlit', 'Stormy', 'Swift', 'Thunderous', 'Tranquil', 'Twilight', 'Valiant', 'Veiled', 'Vengeful', 'Verdant', 'Vivid', 'Whispering'];
const serverNameNouns = ['Abyss', 'Alcove', 'Arcade', 'Archipelago', 'Arena', 'Asylum', 'Barracks', 'Basin', 'Bastion', 'Canyon', 'Cavern', 'Chamber', 'Citadel', 'Colony', 'Corridor', 'Crest', 'Depths', 'Domain', 'Dungeon', 'Enclave', 'Expanse', 'Fields', 'Forge', 'Fortress', 'Frontier', 'Gateway', 'Grotto', 'Hall', 'Harbor', 'Haven', 'Hideout', 'Island', 'Jungle', 'Keep', 'Labyrinth', 'Lair', 'Manor', 'Maze', 'Oasis', 'Outpost', 'Passage', 'Pathway', 'Peak', 'Plateau', 'Prairie', 'Promenade', 'Ridge', 'Rift', 'Sanctuary', 'Sanctum', 'Spire', 'Stronghold', 'Summit', 'Terrace', 'Trench', 'Valley', 'Vault', 'Ward', 'Wasteland', 'Wilderness', 'Zone'];

function generateServerName() {
  const adjective = serverNameAdjectives[Math.floor(Math.random() * serverNameAdjectives.length)];
  const noun = serverNameNouns[Math.floor(Math.random() * serverNameNouns.length)];
  return `${adjective} ${noun}`;
}

const serverNameInput = /** @type {HTMLInputElement} */ (document.getElementById('server-name-input'));
const serverPasswordInput = /** @type {HTMLInputElement} */ (document.getElementById('server-password-input'));
const autoUpdateMapInput = /** @type {HTMLInputElement} */ (document.getElementById('auto-update-map-input'));
{
  const serverId = localStorage.getItem('serverId') ?? window.crypto.randomUUID();
  localStorage.setItem('serverId', serverId);

  const serverName = localStorage.getItem('serverName') ?? generateServerName();
  localStorage.setItem('serverName', serverName);
  serverNameInput.value = serverName;

  const serverPassword = localStorage.getItem('serverPassword') ?? serverId.substring(0, 8);
  localStorage.setItem('serverPassword', serverPassword);
  serverPasswordInput.value = serverPassword;

  const autoUpdateMap = localStorage.getItem('autoUpdateMap') ?? 'false';
  localStorage.setItem('autoUpdateMap', autoUpdateMap);
  autoUpdateMapInput.checked = (autoUpdateMap === 'true');
}

const eventSource = new EventSource('server-events?' + new URLSearchParams({ server_id: localStorage.getItem('serverId') }));
const onlineToast = document.getElementById('online-toast');
eventSource.addEventListener('online', (event) => {
  onlineToast.innerHTML = `The server is online at <a href="ddnet://${event.data}">${event.data}</a>`;
  onlineToast.style.display = '';
});
eventSource.addEventListener('stopped', () => {
  onlineToast.style.display = 'none';
});
eventSource.addEventListener('shutdownwhenempty', () => {
  appendToast('The server has been online for a while and will shut down when empty.', 'lightgray', 8000);
});

serverNameInput.addEventListener('change', () => {
  if (serverNameInput.value !== '') {
    localStorage.setItem('serverName', serverNameInput.value);
    pushSettings();
  }
});
serverPasswordInput.addEventListener('change', () => {
  if (serverPasswordInput.value !== '') {
    localStorage.setItem('serverPassword', serverPasswordInput.value);
    pushSettings();
  }
});
autoUpdateMapInput.addEventListener('change', () => {
  localStorage.setItem('autoUpdateMap', autoUpdateMapInput.checked.toString());
});
window.addEventListener('storage', () => {
  // Update form inputs when changes are made in another tab
  serverNameInput.value = localStorage.getItem('serverName');
  serverPasswordInput.value = localStorage.getItem('serverPassword');
  autoUpdateMapInput.checked = (localStorage.getItem('autoUpdateMap') === 'true');
});

const mapDropZone = document.getElementById('map-drop-zone');
const mapFileInput = /** @type {HTMLInputElement} */ (document.getElementById('map-file-input'));
const autoUpdateMapToast = document.getElementById('auto-update-map-toast');
mapDropZone.addEventListener('dragover', (event) => {
  event.preventDefault();
});
// If available, use the File System Observer API to automatically update the map
if ('FileSystemObserver' in self && 'getAsFileSystemHandle' in DataTransferItem.prototype) {
  // Reveal the checkbox and the enclosing label
  autoUpdateMapInput.parentElement.style.display = '';
  // Disable the file picker because it restricts access to AppData on windows
  function updateDropZoneText() {
    if (autoUpdateMapInput.checked) {
      mapDropZone.innerText = 'Drag a map file here';
    } else {
      mapDropZone.innerText = 'Drag a map here or click to upload a file';
    }
  }
  updateDropZoneText();
  autoUpdateMapInput.addEventListener('change', updateDropZoneText);
  window.addEventListener('storage', updateDropZoneText);
  // @ts-ignore
  const fileSystemObserver = new FileSystemObserver(async (records) => {
    for (const record of records) {
      if (record.type === 'appeared') {
        pushMap(await record.root.getFile());
        break;
      }
    }
  });
  mapDropZone.addEventListener('drop', async (event) => {
    event.preventDefault();
    // @ts-ignore
    const fileHandle = await event.dataTransfer.items[0].getAsFileSystemHandle();
    if (fileHandle instanceof FileSystemFileHandle) {
      pushMap(await fileHandle.getFile());
      if (autoUpdateMapInput.checked) {
        fileSystemObserver.disconnect();
        fileSystemObserver.observe(fileHandle);
        autoUpdateMapToast.style.display = '';
      } else {
        fileSystemObserver.disconnect();
        autoUpdateMapToast.style.display = 'none';
      }
    }
  });
  mapDropZone.addEventListener('click', async () => {
    if (!autoUpdateMapInput.checked) {
      mapFileInput.click();
    }
  });
  mapFileInput.addEventListener('change', () => {
    pushMap(mapFileInput.files[0]);
    fileSystemObserver.disconnect();
    autoUpdateMapToast.style.display = 'none';
    // Trigger another change event when the same file is selected again
    mapFileInput.value = null;
  });
  eventSource.addEventListener('stopped', () => {
    fileSystemObserver.disconnect();
    autoUpdateMapToast.style.display = 'none';
  });
} else {
  mapDropZone.addEventListener('drop', (event) => {
    event.preventDefault();
    pushMap(event.dataTransfer.files[0]);
  });
  mapDropZone.addEventListener('click', () => {
    mapFileInput.click();
  });
  mapFileInput.addEventListener('change', () => {
    pushMap(mapFileInput.files[0]);
    // Trigger another change event when the same file is selected again
    mapFileInput.value = null;
  });
}

async function pushSettings() {
  let response;
  try {
    response = await fetch('update-settings?' + new URLSearchParams({
      server_id: localStorage.getItem('serverId'),
      server_name: localStorage.getItem('serverName'),
      server_password: localStorage.getItem('serverPassword')
    }));
  } catch (error) {
    appendToast(`Could not update the server settings: ${error}`, 'lightsalmon', 8000);
    return;
  }
  if (!response.ok) {
    appendToast(`Could not update the server settings: ${await response.text()}`, 'lightsalmon', 8000);
    return;
  }
  if (response.statusText === 'Accepted') {
    appendToast('The server settings have been updated.', 'lightgray', 4000);
  }
}

/**
 * @param {File} file 
 */
async function pushMap(file) {
  let response;
  try {
    response = await fetch('update-map?' + new URLSearchParams({
      server_id: localStorage.getItem('serverId'),
      map_filename: file.name,
      server_name: localStorage.getItem('serverName'),
      server_password: localStorage.getItem('serverPassword')
    }), {
      method: 'POST',
      body: file
    });
  } catch (error) {
    appendToast(`Could not update the map: ${error}`, 'lightsalmon', 8000);
    return;
  }
  if (response.status === 413) {
    appendToast('Could not update the map: The file size is too large', 'lightsalmon', 8000);
    return;
  }
  if (!response.ok) {
    appendToast(`Could not update the map: ${await response.text()}`, 'lightsalmon', 8000);
    return;
  }
  if (response.statusText === 'Accepted') {
    appendToast('The map has been updated.', 'lightgray', 4000);
  }
}

/**
 * @param {string} text 
 * @param {string} backgroundColor 
 * @param {number} removeAfterMillis 
 */
function appendToast(text, backgroundColor, removeAfterMillis) {
  const toast = /** @type {HTMLElement} */ (onlineToast.cloneNode());
  toast.textContent = text;
  toast.style.display = '';
  toast.style.backgroundColor = backgroundColor;
  document.body.appendChild(toast);
  setTimeout(() => { toast.remove(); }, removeAfterMillis);
}
