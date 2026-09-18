import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile} from 'node:fs/promises';
import {extractLogin} from '../src/detector.js';
import {validCandidate,trustedPopup,webOrigin} from '../src/policy.js';
import {messages} from '../src/messages.js';
const require=createRequire(new URL('../../app/package.json',import.meta.url));
const {JSDOM}=require('jsdom');
function fixture(html){const dom=new JSDOM(`<form>${html}</form>`,{url:'https://example.invalid/'});for(const input of dom.window.document.querySelectorAll('input')) input.getClientRects=()=>[{width:100,height:20}];return dom.window.document.querySelector('form');}
test('captures only one unambiguous username + current password',()=>{
 const form=fixture('<input name="username" value="alice"><input type="password" value="fixture">');
 assert.deepEqual(extractLogin(form),{username:'alice',password:'fixture'});
});
test('rejects signup, change, OTP, card, ambiguous and oversized fields',()=>{
 for(const extra of ['<button>Sign up</button>','<input type="password">','<input autocomplete="new-password">','<input autocomplete="one-time-code">','<input autocomplete="cc-number">','<input name="email" value="other">'])assert.equal(extractLogin(fixture(`<input name="username" value="alice"><input type="password" value="fixture">${extra}`)),null);
 assert.equal(extractLogin(fixture('<input name="username" value="alice"><input type="password" disabled value="fixture">')),null);
 assert.equal(extractLogin(fixture(`<input name="username" value="alice"><input type="password" value="${'密'.repeat(1400)}">`)),null);
});
test('binds capture to top-frame sender, permitted scheme and exact origin',()=>{
 const sender={id:'abc',frameId:0,url:'https://example.invalid/login',tab:{id:1,url:'https://example.invalid/login'}};
 const message={origin:'https://example.invalid',username:'alice',password:'fixture'};
 assert(validCandidate(message,sender,'abc'));
 for(const other of [{...sender,id:'evil'},{...sender,frameId:2},{...sender,url:'https://evil.invalid'},{...sender,tab:{id:1,url:'https://evil.invalid'}}]) assert(!validCandidate(message,other,'abc'));
 assert.equal(webOrigin('http://example.invalid'),null);
 assert(!trustedPopup({...sender,url:'chrome-extension://abc/popup.html'},'abc'));
 assert(trustedPopup({id:'abc',url:'chrome-extension://abc/popup.html'},'abc'));
});
test('manifest is Edge-only with default HTTPS access and complete locales',async()=>{
 const manifest=JSON.parse(await readFile(new URL('../dist/manifest.json',import.meta.url)));
 assert.equal(manifest.manifest_version,3);assert.deepEqual(manifest.host_permissions,['https://*/*']);assert.equal(manifest.optional_host_permissions,undefined);assert(!manifest.permissions.includes('activeTab'));assert(!manifest.permissions.includes('tabs'));assert(!manifest.externally_connectable);
 assert.deepEqual(Object.keys(messages.en).sort(),Object.keys(messages['zh-CN']).sort());
});
