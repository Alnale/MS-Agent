import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import 'katex/dist/katex.min.css'
import './index.css'
import App from './App.tsx'
import { ErrorBoundary } from './components/ErrorBoundary'

// Suppress browser extension connection errors
window.addEventListener('error', (event) => {
  if (event.message?.includes('Could not establish connection') ||
      event.message?.includes('Receiving end does not exist')) {
    event.preventDefault()
    return false
  }
})

// Also suppress unhandled promise rejections from extensions
window.addEventListener('unhandledrejection', (event) => {
  if (event.reason?.message?.includes('Could not establish connection') ||
      event.reason?.message?.includes('Receiving end does not exist')) {
    event.preventDefault()
    return false
  }
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
)
