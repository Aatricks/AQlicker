function App() {
  return (
    <main className="app-shell">
      <header>
        <h1>AQlicker</h1>
        <p className="status" role="status">
          Ready
        </p>
      </header>

      <p>Add a key before a clicking session can start.</p>

      <section aria-label="Clicking mode">
        <h2>Mode</h2>
        <div className="segmented-control" role="group" aria-label="Clicking mode">
          <button type="button" aria-pressed="true">
            Timer
          </button>
          <button type="button" aria-pressed="false">
            Natural
          </button>
        </div>
      </section>

      <button className="start-button" type="button" disabled>
        Start
      </button>
    </main>
  );
}

export default App;
