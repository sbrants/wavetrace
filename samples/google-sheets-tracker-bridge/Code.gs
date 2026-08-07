/**
 * WaveTrace / Tracker Bridge — Google Sheets sample
 *
 * Extensions → Apps Script → paste these files, then reload the sheet.
 * Menu: Tower Save → Open sidebar
 *
 * The sidebar runs in the browser and talks to local tracker-bridge over
 * WebSocket (ws://127.0.0.1:43781). Server-side UrlFetchApp cannot reach localhost.
 *
 * Prerequisites:
 *   npx tracker-bridge
 *   Emulator/phone online with The Tower save
 *   Allow local network access if Chrome prompts
 */

var BRIDGE_DEFAULT_URL = 'ws://127.0.0.1:43781';

/** Adds a custom menu when the spreadsheet opens. */
function onOpen() {
  SpreadsheetApp.getUi()
    .createMenu('Tower Save')
    .addItem('Open sidebar', 'showTowerSaveSidebar')
    .addToUi();
}

/** Opens the Tracker Bridge sidebar. */
function showTowerSaveSidebar() {
  var html = HtmlService.createHtmlOutputFromFile('Sidebar')
    .setTitle('Tower save')
    .setWidth(320);
  SpreadsheetApp.getUi().showSidebar(html);
}

/** Sidebar health check (no ADB / no WebSocket). */
function sidebarPing() {
  return 'pong ' + new Date().toISOString();
}

/**
 * Called from the sidebar after a successful PULL_SAVE.
 * @param {Object} payload
 * @param {string} payload.base64 - gzip playerInfo.dat as base64
 * @param {number} payload.byteLength
 * @param {string} payload.remotePath
 * @param {string} payload.deviceSerial
 * @param {string=} payload.deviceLabel
 * @return {Object} summary written to the sheet
 */
function receivePulledSave(payload) {
  if (!payload || !payload.base64) {
    throw new Error('No save data received from the sidebar.');
  }

  var ss = SpreadsheetApp.getActiveSpreadsheet();
  var sheet = ss.getSheetByName('TowerSave') || ss.insertSheet('TowerSave');

  var bytes = Utilities.base64Decode(payload.base64);
  var blob = Utilities.newBlob(bytes, 'application/octet-stream', 'playerInfo.dat');

  // Drive file in the user's Drive (optional keep); also log metadata on the sheet.
  var folder = DriveApp.getRootFolder();
  var file = folder.createFile(blob);
  file.setName(
    'playerInfo-' +
      Utilities.formatDate(new Date(), Session.getScriptTimeZone(), 'yyyyMMdd-HHmmss') +
      '.dat'
  );

  var row = [
    new Date(),
    payload.deviceSerial || '',
    payload.deviceLabel || '',
    payload.remotePath || '',
    payload.byteLength || bytes.length,
    file.getUrl(),
  ];

  if (sheet.getLastRow() === 0) {
    sheet.appendRow([
      'Pulled at',
      'Device serial',
      'Device label',
      'Remote path',
      'Bytes',
      'Drive file',
    ]);
  }
  sheet.appendRow(row);

  return {
    ok: true,
    bytes: payload.byteLength || bytes.length,
    driveUrl: file.getUrl(),
    sheetName: sheet.getName(),
  };
}
