import './index.css';
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.jsx';
import ErrorBoundary from './components/ErrorBoundary.jsx';
import { I18nProvider } from './i18n';

const isLinux = /Linux/i.test(`${navigator.platform} ${navigator.userAgent}`);
document.documentElement.classList.toggle('platform-linux', isLinux);

ReactDOM.createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <I18nProvider>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </I18nProvider>
  </React.StrictMode>,
);
