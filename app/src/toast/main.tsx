import { createRoot } from "react-dom/client";

import "../styles/tokens.css";
import { Toast } from "./Toast";
import "./toast.css";

createRoot(document.getElementById("root")!).render(<Toast />);
