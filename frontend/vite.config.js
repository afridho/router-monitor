import {defineConfig} from 'vite';
export default defineConfig({server:{proxy:{'/api':{target:'http://127.0.0.1:8787',changeOrigin:true,configure(proxy){proxy.on('proxyReq',req=>{req.setHeader('origin','http://127.0.0.1:8787');});}}}}});
