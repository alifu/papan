import "./css/App.css";
import Layout from "./Layout";
import Box from "./Box";

function App() {

  return (
    <main className="container">
      <Layout>
        <Box>
          <h1>Main Content</h1>
        </Box>
      </Layout>
    </main>
  );
}

export default App;
